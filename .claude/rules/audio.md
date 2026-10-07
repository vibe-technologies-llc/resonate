---
paths:
  - "crates/resonate-engine/**/*.rs"
  - "crates/resonate-codec/**/*.rs"
  - "crates/resonate-dsp/**/*.rs"
  - "crates/resonate-pipewire/**/*.rs"
---

# The audio path

Invariants from file to sink. Callback contract: `realtime.md`.

## Decode

- **Depth: container, never decoder buffer.** symphonia leaves ALAC `bits_per_sample` unset;
  `container.rs` reads the magic cookie (else 24-bit ALAC says 32 and `plan_output` asks the sink
  for a format the file never had). `declared_bits` asks each container, nearest the codec first:
  coded width, WAV `wValidBitsPerSample`, FLAC `STREAMINFO`, Matroska `BitDepth`,
  `bits_per_sample`. The inspector prints the made-at depth; padding lies about the master.
- **Layout named only where the container places every channel as that layout does.**
  `positioned_layout` weighs symphonia's speaker mask against named layouts; else (6.0, LCRS)
  `ChannelLayout::Discrete`, not the same-count layout (a downmix reads a 5.1's fourth channel as
  LFE and drops it). 1-2 channels are mono/stereo wherever placed; more, unplaced: `Discrete`.
- **Tags read under the writer's naming, not only the container spec's.** symphonia maps raw keys
  via the container vocabulary, so ffmpeg's Matroska (`ALBUM`, `ALBUM_ARTIST`, `DATE`) reached the
  `TagSet` through nothing. `tags::Naming` decides from the key set: one `VORBIS_COMMENT_ONLY` name
  (names no container spec defines; `DATE`, `BPM`, `ISRC`, `LABEL` absent) = Vorbis comments.
  What a reader never exposes is taken off the source first (`riff.rs`: WAV `INFO`, `id3 ` chunk;
  `matroska.rs`: segment `Title`, filling a track only if the container holds one audio track);
  `Prescan` bundles both. A WAV's `id3 ` revision is appended *last*, outranking `INFO` and a
  leading tag. `IDIT`/`DTIM` are not read from `INFO` (digitisation dates, not release).
- **`TagSet` is the vocabulary; other names stay `RawTag`.** Every well-known name has a typed
  field, all three naming paths land on them. Omitted on purpose: RIFF `ISRC` (names the *source*),
  `EncodedBy` (who, not what: `encoder` takes `Encoder` alone), album artist from `INFO` alone
  (`store::attribution` falling back to track artist is the answer).
- **A name given twice in a revision is every one of them.** `Builder::listed` handles list fields
  (artist, album artist, genre, credits): a revision's first replaces older revisions' value, later
  ones append after `LISTED_APART_BY` unless held. symphonia gives multi-valued ID3 frames as
  `RawValue::StringList` with no standard tag; `id3_list` maps `TPE1`, `TPE2`, `TCON`, `TCOM`,
  `TPE3`, `TEXT`, `TPE4` itself (`TMCL` pairs become performers). Title, album, id, number: last
  wins.
- **Text blank once trimmed is no tag; stored text is trimmed.** `tags::given` is the one door for
  text `StandardTag`s into a `TagSet` slot, leaving it unchanged if nothing remains (a blank frame
  after a name must not unname it). ReplayGain values and numbers likewise: unparsable, zero or
  oversized keeps the earlier frame's value (an empty frame must not play at no gain).
  `tags::decibels`/`tags::peak` take a unit in any case and a decimal comma where it is the only
  separator. Newest revision wins a retagged date; ID3v2.3 `TDAT`/`TIME` read by own key
  (`Id3DatePart`), clock never a date. `COMM` description beginning `iTun` = iTunes private note
  (`is_an_itunes_note`).
- **Gain heard = declared ReplayGain moved to our reference, else Sound Check, else encoder's.**
  `TagSet::replay_gain` stays as written (tag writing spells it back); `TagSet::heard_gain` is what
  engine, `info`, inspector and catalog `rg_*` columns read. Declared track/album gain is raised by
  89 dB less `REPLAYGAIN_REFERENCE_LOUDNESS` (`reference_loudness`: `dB`, or `LUFS`/`LKFS` read
  107 dB lower, -18 LUFS = no change; outside 60-120 dB ignored). Neither declared: iTunes Sound
  Check (`iTunNORM`; ID3 `COMM` note, MP4 free-form item from box scan; ten fields; gain = louder
  of first two as `-10*log10(v/1000)` dB, peak = louder of fields 7/8 over 32 768), then LAME
  header radio/audiophile gains (`mpa::encoder_gain`, set ones only, peak 8.23 fixed point).
  Tag-declared peaks stand beside a fallback's gain
  (`a_gain_aimed_at_another_reference_is_heard_against_ours_and_kept_as_written`,
  `sound_check_levels_a_track_naming_no_replay_gain_and_never_one_that_does`,
  `the_encoders_gain_comes_after_sound_check_and_the_tags`,
  `the_gains_a_lame_header_names_are_read_and_an_unset_one_is_not`).
- **ID3 recording id: `UFID` frame, no standard tag.** `musicbrainz_identifier`: key, owner =
  `MUSICBRAINZ_OWNER` (whole check; CDDB et al. use `UFID` too), bytes as UTF-8; only where
  `Naming::standard` answered nothing.
- **Matroska length: segment's, counted as well as read.** symphonia leaves `duration` unset.
  `matroska.rs` reads `Duration`, `TimestampScale` and walks clusters (streamed file without
  `Duration` still has a length). Count is a deliberate *lower* bound: `Segment::duration` keeps
  the declaration while clusters stay within it, takes the count only with blocks past the declared
  end. Too-long declarations are detectable only for the first `A_OPUS`/`A_FLAC`/`A_VORBIS` track,
  counted exactly (`Segment::counted_frames_of`) where it is the track symphonia decodes. **Vorbis
  packet length is named by the previous packet** (`vorbis::Windows`: modes walked *backwards* from
  the setup header's framing bit, as ffmpeg's `vorbis_parser`). Seeks are exact only where track
  timebase = reciprocal of sample rate; Matroska's ms ticks make landing approximate.
- **Opus: `opus-rs`, registered beside symphonia's decoders.** symphonia 0.6 demuxes Opus, decodes
  none; `opus.rs` wraps `opus-rs` (pure-Rust libopus port) as `AudioDecoder`; `registry::codecs`
  is the `CodecRegistry` every decoder is built from. `Head` reads the identification header from
  extra data (Ogg/Matroska `OpusHead` little-endian, MP4 `dOps` big-endian). Family 1 = multistream
  decoder, Vorbis-ordered channels written to the plane each position takes in symphonia's bit
  order; Matroska leaves surround unplaced, so `opus::channels_of` places family 1 by its head (255
  stays discrete). Header output gain applied as samples are written; `R128_TRACK_GAIN`/
  `R128_ALBUM_GAIN` read as ReplayGain 5 dB louder. **Pre-skip is the container's:** Ogg counts it
  *inside* granules, Matroska shifts timestamps by `CodecDelay` (music at zero), MP4 edit list is
  read by the box scan (`container::Carrying`). **Matroska's end = last block's `DiscardPadding`**
  (symphonia reads it nowhere): cluster walk keeps it, `Decoder::build` hands it to
  `Coded::trailing`, `fill` reads a packet ahead and trims the last. **Declared length is
  counted:** each packet's TOC byte gives its length, so `Segment::opus_music` is `playable`'s end
  and length = exactly what decodes; trusted only if nothing escaped the count (bad lace, packet
  over 120 ms, segment not walked to its end), else window stays open-ended and length = ms count
  less pre-skip (`Carrying::declared_before_the_music`). **Seeks start early** (`PRE_ROLL`, 19 200
  frames; CELT predicts band energy from the previous frame): `seek_reader` subtracts
  `opus::pre_roll`, discards those frames.
- **WavPack: read by `symphonia-codec-wavpack`, decoded through the codec crate's wrapper.**
  `registry.rs` holds the one `Probe` and `CodecRegistry` every open uses. The crate mishandles
  extended bits of 32-bit/float blocks: `wavpack.rs` rewrites each packet first and converts the
  answered integers to floats itself (port of libwavpack `float_values`; shifts held to a word,
  `SHIFTED_WITHIN_A_WORD`). **Hybrid is billed as the lossy codec it decodes as:** prescan reads
  the first block's flags; `HYBRID` makes `coded_info` answer `wavpack::HYBRID_CODEC_ID`, read by
  `Codec::from_id` as `Codec::WavPackHybrid` (not `is_lossless`; badge, `is:lossy`, verdict, vault
  `Kept` agree); decode unchanged. `.wvc` correction files are not read
  (`symphonia-codec-wavpack` 0.1.1 gets a held zero's correction wrong).
- **Vorbis setup header walked before symphonia's decoder reads it.** An *ordered* codebook can
  count lengths past 32 and index past symphonia's table (aborts the player in release).
  `vorbis::Vorbis` wraps `VorbisDecoder`, walks every codebook, refuses where a run passes 32.
- **Monkey's Audio: own reader.** `ape.rs` `ApeReader` parses header and seek table via
  `ape-decoder`, one packet per frame, seeks by frame. Markers `MAC ` and `MACF` (11 floating;
  without it the probe opened the stored `RIFF` header as WAVE). `Ape` decodes via `FrameDecoder`
  (per-frame checksum); 32-bit stereo refused (crate narrows the side channel before undoing it).
- **WAVE too long for RIFF: own reader** (symphonia reads neither RF64/BW64 `ds64` nor Sony
  Wave64). `wide.rs` `WideReader` walks either to `data` (`Wide::chunk`: the one chunk-header
  reading for both layouts), reads `fmt `, packets of at most `FRAMES_A_PACKET` frames and
  `MOST_PACKET_BYTES`, exact seeks; a data size past a source that states its length is cut to
  bytes held. `riff.rs` walks the same layouts. Scan takes `.rf64`, `.w64` beside `.wav`.
- **Every PCM symphonia names = `Codec::Pcm`; ADPCM = `Codec::Adpcm`** (lossy, not
  `is_lossless`). **64-bit float decodes as 32-bit float:** `container::representable` maps `F64`
  to `SampleFormat::F32` (more than any converter records); declared depth stays 64 so `analyse`
  and the inspector say what the file is.
- **This build drops every priming itself.** `Decoder::build` makes its decoder with
  `AudioDecoderOptions::gapless(false)`; only `MediaInfo::playable` trims (only
  `symphonia-bundle-mp3` and `symphonia-codec-vorbis` implement symphonia gapless).
- **Priming reaches `playable` from whoever named it, reader or `boxes.rs`.** `container::priming`
  prefers `Track::delay`/`padding`/`num_frames`, else the box scan. isomp4's reader names nothing:
  scan reads the `iTunSMPB` item, else first non-empty `elst` edit weighed against what `stts` says
  the decoder emits; `soun` track only, media timescale = sample rate only. iTunes MP3 notes it in
  a `COMM` frame (`tags::itunes_gapless_note`, taken last by `container::itunes_priming`);
  `Priming::behind_a_decoder_delay` adds `MP3_DECODER_DELAY` to delay, subtracts it from padding.
- **Fragmented MP4 length is in the fragments, not `moov`.** DASH-style files (empty `mvhd`,
  `mdhd`, sample tables; audio in `moof`/`mdat`) declare nothing. Box scan reads `mvex/mehd`'s
  fragment duration, or the summed subsegment durations of the first top-level `sidx`, rescaled to
  the stream rate; `coded_info` takes `Movie::fragmented_length` when declared length is zero,
  before the Matroska segment's.
- **`playable` = decoded frames, possibly open-ended.** `Decoder::build` drops `playable.start()`
  frames before the first block, limits at `playable.frames()`; `duration` = playable count (seek
  bar, `mpris:length`, `heard.rs`). `Priming` holds the count as `Option`: a reader can name a
  delay without a length (ogg over a pipe; Xing header with LAME delay, no frame count) and the
  priming must still go.
- **MP3 naming no length is counted where bitrate moves.** symphonia reckons a stream with no
  Xing/Info count and no VBRI from its first sixteen frames' bitrate (off by half for VBR).
  `mpa::counted_frames`, run by `Prescan::buffered` over a whole source only (spooled head never
  counted): leaves streams whose header names frames to the reader; weighs first 32 frames plus
  eight resynced at even steps through the file, leaving constant rate to the reader; else walks
  every frame header of the stream's own version/layer/rate, resyncing over junk on two chained
  headers within 64 KiB, stopping at ID3v1, APEv2 or Lyrics3 trailer. `coded_info` takes the count
  before the reader's. A seek past the reader's shorter reckoning (symphonia `OutOfRange`) lands on
  its last frame and decodes on (`past_the_readers_own_end`). `mpa::FrameHeader` is the one MPEG
  header parser (junk search too)
  (`a_variable_rate_mp3_naming_no_length_is_as_long_as_it_decodes_and_seeks_to_its_end`).
- **`Timeline` holds where the music starts, not the priming length.** One
  `music_at: Timestamp`; playable frame -> container timestamp by adding to it: zero where the
  reader named the delay (symphonia negative PTS), else `Track::start_ts` plus scanned priming (Ogg
  sets `start_ts` to `-delay`; carrying them separately was wrong). A seek can land *ahead* of
  `music_at`: `seek_reader` answers a `Landing` with `short_of_the_music`, `restart` drops it. On
  a non-sample-accurate timeline (Matroska) a landing on the first packet drops
  `MediaInfo::priming` whole, as the cold open does.
- **Every metadata revision is read.** Containers publish several (Matroska one per `Tags`
  element, isomp4 two, AIFF two); symphonia `skip_to_latest` discards the rest. `tags::Revisions`
  drains the log once in `container::open`, oldest first, each revision absorbed alone so `Naming`
  weighs one writer's keys. Visuals stay in the reader's log for `probe_cover_art`.
- **A picture is found once, copied once; the caller says who copies.** `probe::picture` is the one
  walk, handing results to the caller's `taken`. **`probe_pictured` is the one open answering tags
  and picture together**; the caller's `Picturing`: `Whether` copies nothing, `Copied` answers
  bytes; a row the sources stand in for answers `Pictured::StoodIn`. Library scan uses
  `probe_scanned` (`Whether`; why: `library.md`) and digests the first `PACKETS_DIGESTED` (48)
  packets into a `PacketDigest` on the same open (identifies a moved file once retagged).
  `TagSource::read` takes the `Picturing`: no implementation can read tags and picture through
  separate opens.
- **Engine tag reader takes the picture on the tags' open where it can.** `catalog::whole` probes
  under `Copied`; `Shelf::keep_what_the_tags_saw` files bytes where the picture is already waited
  on, `Look::Nothing` where there is none, else into `Spares` (at most `SPARE_ART_BYTES`, outside
  `ART_BYTES_HELD` until drawn). A row a sheet cuts says nothing about the picture.
- **Engine catalog follows the file.** Failed read = `Look::Failed`, reclaimed after
  `READ_AGAIN_AFTER`; `Nothing` = read found nothing, stands. Each entry keeps file size+mtime
  (`Stamp`), taken by the reader thread *before* reading, weighed at most every
  `LOOKED_AT_THE_FILE_EVERY`: a due row is queued by the lookup (`Held::to_look`, sent after the
  lock is released, `Wanted::Look`); the reader thread stats it, forgets a changed file and bumps
  the revision so a pane asks again. Nothing stats under the lock or on the caller's thread: a
  stalled mount stalls the reader alone; redraw and bus go on with what was held.
- **Pictures are weighed before copying.** `MAX_COVER_BYTES` (24 MiB) is checked against
  `data.len()` before `to_vec`; oversized visuals are declined, not read failures. `choose`
  prefers front cover, then `Other`/untyped. Icon, leaflet, back cover, disc, portrait are never
  the cover; a file with only those answers `None`.
- **Hand-rolled parsers never allocate what a header declares.** `riff.rs`, `matroska.rs` read
  `declared.min(MAX_…_BYTES)` (`id3 ` chunk via a `take` growing only as bytes arrive), then seek
  absolutely past the claimed element. A declared size is what the walk advances by, never a
  `Vec`'s size.
- **Non-seekable sources are spooled; only one too long to spool is read from its head.**
  `container::open` reads it into memory up to `SPOOLED_AT_MOST` (256 MiB) for at most
  `SPOOLED_WITHIN` (750 ms); ending inside both, it opens as seekable `Reading` with all a file
  has. The wait stops a slow remote stream holding the first sample for its whole download; a
  stream still arriving goes on arriving onto disc. `spool::Spool` = unnamed file (unlinked at
  once) under `TMPDIR` if set else `/var/tmp` (`/tmp` is tmpfs on Arch, Fedora, Flatpak),
  `env::temp_dir()` fallback, holding the head; a `resonate-spool` thread copies the rest up to
  `SPOOLED_ON_DISC_AT_MOST` (8 GiB). symphonia gets the `Spooling` side (waits for the rest, still
  `is_seekable() == false`: a reader that thinks it can seek looks for the end at once); walks run
  over a `Cursor` of the head. **Once all has arrived it can seek:** `Decoder::settle_the_spool`
  answers `None` until the copy ends, then reopens the spooled file as seekable `Unspooled`, seeks
  to the old decoder's frame, takes its place; engine asks every pass
  (`Engine::settle_what_was_spooled`), republishes the digest so MPRIS `CanSeek` follows. Spool
  not makeable: fall back to `Replaying` head; one cut short never claims to be whole. DSF, DSDIFF,
  Monkey's Audio are read by seeking: one too long for memory is spooled on disc and the open waits
  for all of it (`spooled_whole_on_disc`, `Spool::ended_whole`, on the `resonate-track-open`
  worker), opening seekable `Unspooled` with no spool left to settle; only where the disc spool
  cannot be made, or is cut short at `SPOOLED_ON_DISC_AT_MOST`, `Error::ReadBySeeking`
  (`a_dsd_pipe_too_long_to_hold_in_memory_is_spooled_whole_on_disc_and_seeks`,
  `a_dsd_or_monkeys_audio_pipe_too_long_even_for_the_disc_is_named_as_wanting_a_seek`).
- **Until it can seek, a track keeps its stream.** A rebuild discards ring and carry and seeks the
  decoder back, impossible for such a track. `Engine::seek` refuses with
  `codec::Error::NotSeekable` (`Cause::CannotSeek`), except a seek to its start (restarting
  Previous too), which reopens the row via `start`. A stream-rebuilding setting goes through
  `rebind_where_it_stands`, marking the rebuild owed; `settle_what_was_spooled` pays it the pass the
  spool settles, the next track's `start` forgets it. Graph-forced rebuilds (device gone, format
  renegotiated, stalled discard) still happen at once.
- **Prescan over a seekable source reads through a window** (walks read ids/headers four bytes at a
  time): `Prescan::buffered` = `Prescan::read` over a `Window`, one `PRESCAN_WINDOW` array
  refilled by absolute seek+read, free `stream_position`; `rewind_the_source` restores the stream.
- **What can crash symphonia is guarded where the prescan sees it.** Its WAVE reader multiplies a
  `fmt ` channel count into `u16` and widens a short channel mask by a shift past 32 bits (wraps in
  release, panics in debug/fuzz). `container::open` refuses more channels than a WAVE layout holds
  (`Error::TooManyChannels`) or a mask `riff::a_mask_the_decoder_cannot_widen`
  (`Error::ChannelMaskNotRepresentable`) before symphonia sees either. symphonia's probe finds
  magic byte by byte, so `prescan::opened_first` searches as far (`PROBED_WITHIN`); guards weigh
  only `RIFF … WAVE` and `caff`, standing aside for other containers. `caf.rs` reads the CAF
  reader's declared values first (`Error::PacketTooLarge`, `FrameCountNotRepresentable`,
  `PacketOffsetNotRepresentable`, `PacketTooLong` over `MOST_FRAMES_A_PACKET`, `PacketTableCut`).
- **A cue track is a window on a file; the window lives in the decoder.** `Decoder::open_span`
  seeks to the span's first frame, stops at its last, rewrites `MediaInfo::duration` to the span
  length: upstream (seek clamp, seek bar, `heard.rs`, `mpris:length`) sees a short file;
  `self.position` stays absolute inside, only `position()`/`seek()` convert. `probe_span` = `probe`
  through the same cut, so the tags a queue row draws and plays under come off one reading. A
  sheet's 1/75 s unit is a `sector`, never a frame (`Frames` = PCM frame; `CueStamp::at` is the one
  conversion).
- **A window is billed by the sheet that cut it; the span names the row.** `confine` writes the
  row's own `TagSet` over the file's where a cut names the span's start frame (title, number,
  `REPLAYGAIN_TRACK_GAIN`). `CueFile::cut_at` matches the start frame alone (heard-from or
  written); `CueTrack::titled` and scan rows use the same call (catalog and transport cannot bill
  a row two ways). `cue::cut_for` finds the cut: the file's embedded `MediaInfo::cue`, else the
  sheet beside a *local* file (`<stem>.cue`, then `<name>.cue`, via `sheets_beside`) whose `FILE`
  line names it as scan matching does (`library.md`) or whose only cut it is, else a sheet in one
  of the `DEEPEST_FOLDER_A_SHEET_NAMES` folders above whose `FILE` line reaches down to it
  (`cue::above`, first `MOST_SHEETS_ABOVE`).
- **Track start is a type.** `CueTrack::start` is a `CueStart`: `Written(CueStamp)` for text
  sheets, `Sampled(Frames)` for a FLAC `CUESHEET` block (never rounded to a sector).
  `MediaInfo::cue` = Vorbis `CUESHEET` comment, else the binary block `flac.rs` walks. Blocks have
  no names: `CueFile::billed_by` bills tracks by the file's album fields. Lead-out track is kept as
  `CueTrackKind::Data`: `audio_tracks` skips it, `span_of` reads its offset as the last audio
  track's end.
- **File chapters cut like an embedded sheet.** `container::chaptered` is `MediaInfo::cue`'s third
  source: symphonia `FormatReader::chapters` (MP3 `CHAP`, Ogg `CHAPTERnnn`), else prescan reads
  (`boxes.rs`: m4b QuickTime chapter track or Nero `chpl`), else `flac.rs` FLAC `CHAPTERnnn`
  comments (symphonia discards them). `chapters::cut` turns two or more distinct starts into a
  `CueFile` of `CueStart::Sampled` tracks; one chapter, or all at one moment, cut nothing.
  **Matroska chapters are read by the prescan, hidden from symphonia**, whose reader refuses an
  `EditionEntry` without `EditionUID` (optional in the spec, omitted by ffmpeg):
  `matroska::read_chapters` takes them, `Voided` rewrites the element header as an EBML `Void` over
  the bytes symphonia reads via `container::Probed`.
- **Sheet edge cases, settled once.** A track belongs to the file its `INDEX 01` is in, whichever
  `FILE` line its `TRACK` followed (EAC gaps-appended sheets: `Reading::file` carries the track
  across, start reset to the new file's head). A pregap inside one file heads the track it leads
  into (`CueTrack::heard_from` is the one answer to where a row begins; `PREGAP` is not read). A
  track whose `INDEX 01` does not read, or that starts before the one ahead of it in its file, is
  dropped (no two rows of a file overlap). A `FILE` line may name a folder below its sheet
  (`cue::folder_named`, at most two deep, any case; scan, organiser, cue reader all ask it).
  Sheet-level `REM DISCNUMBER`, `TOTALDISCS`, `COMPOSER`, `SONGWRITER`, `CATALOG` go to every track
  naming no own; a quoted value runs to the *last* quote on its line.
- **A cue sheet is read as far as it parses and never fails.** Unknown commands skipped; rubbish
  yields a sheet naming nothing. Only the source and `LARGEST_CUE_SHEET` raise a `codec::Error`.
  Text decoded by `resonate_core::text` (`rust-style.md`); a renamed sheet is written back in the
  code page it was read in.
## DSD

- **Carrier rate is what the workspace sees, DoP or decimated.** `SampleRate` caps at 768 kHz
  (`MAX_HZ`), so a DSD rate never fits: `DsdRate` holds the native rate, `carrier()` = `hz / 16`.
  DSD64 = 176.4 kHz `S24` stereo either way; `Frames`, `Timeline`, seek bar, ring, `plan_output`
  need no special case. DSD512's carrier is out of range: `RateNotRepresentable` at open.
- **`Packing` is the guard.** On `MediaInfo` and `OutputPlan`; `untouched`, sole constructor of
  `DopMarked`, writes `resample`, `equalisation`, `gain`, `dither_to`, `shaping` as literals: no
  path from packed source to staged plan
  (`no_setting_the_pane_offers_can_produce_a_marked_plan_with_a_stage_in_it`: every sink shape x
  every setting). `OutputPlan::delivery` derives the decoder's request from the same plan.
- **DoP is opt-in, off by default** (`EngineConfig::dop`): ALSA/SPA advertise nothing; DoP into a
  DAC that does not decode it is full-scale white noise. Gate: `SinkInfo::supports` exactly, never
  `best_spec_for` (widest-format fallback truncates DoP to S16 noise; layout fold rewrites markers).
  `Decoder::set_output_format` = *samples in this format*, clears packing; DoP only via `deliver`.
  `resonate explain` prints which was chosen and why not the other.
- **All-zero DSD byte is negative full scale, not silence.** Answers: DSF final-block padding
  truncated against declared sample count; decimator FIR history primed with `DSD_SILENCE` (`0x69`);
  seek gap filled from `Packing::silence`, not PCM zeros.
- **The ring is told what wire silence is.** `ring` takes a `Silence` (`Unmarked`, or `Marked`:
  four-byte word + alternate); `Silence::write` swaps them per frame, so the DoP marker keeps
  alternating through a gap. Sole constructor `Packing::silence` (`DopMarked`: `0x05`/`0xFA` over
  `0x69 0x69`, so a DAC holds lock through a seek's prime window; `Unmarked` writes nothing).
  `Silence` is `Copy`, allocation-free (`RingConsumer::fill` is the RT thread). **A gap is a gap
  however opened:** ring dry mid-track hands the graph a short chunk, losing a DoP DAC's lock, so
  `fill` pads the rest of the quantum after an underrun, except at a track's *end*
  (`Discard::finished`, set when the last audio is written). `RtFault::Underrun` raised either way.
- **Starve cost is counted in frames, apart from faults.** Ring adds frames asked for and not handed
  to one `AtomicU64` (an add: realtime-safe), never for a track's ending short chunk;
  `RingMonitor::went_without` swaps it out, added *before* the fault is raised.
  `OutputStatus::went_without` = total at sink rate; `Event::Underrun` carries frames missed since
  the last one told. Exact where the fault queue is not (no room: bare count in `RtFault::Dropped`),
  hence `RtFault::Underrun` has no frames.
- **Splice marker is the audio's; both gap edges exact.** DoP marker alternates by the decoder's
  *absolute* frame, `Silence` by its own count: silence between two audio frames could put two like
  markers together. Marker read off audio by *sign* (`dop::packed`: `0x05` in a 24-bit word's top
  byte, `0xFA` sign-extended; `marks_negative` reads any width; a hex dump looks right either way,
  the mistake shows once `retype` or `convert` touches the sample). `Silence::follows` = gap's
  leading edge, `RingConsumer::realigned` = trailing: peek first audio frame via uncommitted
  `read_chunk`, write one more silence frame ahead where `Silence::would_write` says its marker is
  not the one due.
- **Decimation is at the modulator's level unless PCM's is asked for.** DSD full scale = 50 %
  modulation: a master decimates ~6 dB quieter than the same PCM music. `EngineConfig::dsd_like_pcm`
  (`dsd-like-pcm`, off): `plan_for` adds `DSD_MODULATION_DB` to gain wherever it decimates, capped
  at a known peak, ridden by the true-peak guard where none known. DoP plan never raised;
  `Command::SetDsdLikePcm` retunes in place.
- **Decimation is a conversion; DoP returns once nothing blocks it.** Decimated stream = carrier
  rate at `S24` = `MediaInfo.spec`, so `plan_for` reads the source's `Packing`, not its spec:
  `DopMarked` delivered as samples = `OutputMode::Converted`, no `NO_CONVERT`. `retune` asks
  `packs_again` (source DSD, open plan samples on carrier's own spec, `dop_survives` against bound
  sink), rebinding into the marked plan where it holds.
- **DST-compressed DSDIFF unpacks into the stream an uncompressed one would be.** `DST ` chunk's
  `FRTE` = frame count (absent: `DsdChunkMissing`), so layout is known from the header. Only a
  decode walks the chunk: `dsd::unpacked` hands the DSD path `dst::Unpacked`, a `MediaStream`
  decoding whichever frame a read lands in; reader, DoP, decimator, seek see an uncompressed DSDIFF.
  **Frame index found as reached, never up front:** `dst::Walk` resumes over `DSTF` headers
  (`dff::next_packed`) only as far as a read or seek asks (open costs no header; rebind costs the
  walk to where it lands; a frame behind the walk is never re-walked). Length = `FRTE` count until
  the walk ends, then frames found. A frame decodes alone: seek costs one frame beyond the walk.
  `dst.rs` = ISO/IEC 14496-3 subpart 10 as FFmpeg's decoder reads it (at most `MOST_CHANNELS`; a
  frame whose segmentation is not the reference encoder's is refused as FFmpeg refuses it); tests
  carry their own encoder.
- **An undecodable packet plays as the silence it would have lasted.** `DecodeError` gives `fill`
  the packet's length via its `PacketSpan`; frames marked `silent`: stream keeps its length,
  position its clock. **Holes are counted; all-holes streams refused:** `Decoder::holes` answers a
  `Holes`; `SILENT_PACKETS_BEFORE_REFUSING` failures with no packet ever decoded =
  `Error::PacketUndecodable`. `Decoder::refuse_holes` makes the first hole that error, for readers
  needing every sample: the vault calls it on what it keeps and reads back, so a damaged source is
  refused `NotValidated` and `--verify` fails an object with a hole.
- **A DSD file's tags are read whole.** ID3 (DSF metadata pointer, DSDIFF `ID3 ` chunk) read to its
  header's declared length, up to `MAX_METADATA_BYTES` (a flat cut made symphonia refuse a tag with
  a large cover, title and all); DSDIFF `DIIN` `DIAR`/`DITI` fill only what ID3 left empty
  (`MAX_EDITED_TEXT_BYTES`). `dsd::described` answers tags and visuals together;
  `container::Pictures` is what every picture question asks. A failed DSD read = `Error::Io`
  reaching `next_block`: engine reports and skips the row, not takes it as finished.
- **Channels are placed where the file says, never by count.** DSF `fmt ` names a channel type,
  DSDIFF `CHNL` an id per channel; `dsd::placed` makes a layout via `container::positioned_layout`.
  Ids out of order (`dsd::in_order`), an unlisted id, or a DSF type disagreeing with its count give
  an unnamed layout by count (mono, stereo or `Discrete`); planes never reordered.
- **DSF bit-reverses, DFF does not.** DSF `fmt ` declares `1` LSB-first, `8` MSB-first; DFF always
  MSB-first. Wrong order = distorted music, not an obvious failure, so a test decodes both orders to
  the same tone.
- **The decimator is the codec's own** (layering forbids `resonate-dsp`): Kaiser-windowed sinc,
  decimate by 16, 512 taps at DSD64, **kernel growing with rate** (1 024 at DSD128, 2 048 at DSD256,
  `fir_bytes_for`): transition band as narrow in hertz, audio band as flat
  (`the_kernel_grows_with_the_rate_so_the_audio_band_stays_flat_at_every_one`); measured a fifth of
  a core for DSD256 stereo. Byte-indexed table of partial sums (no multiplies). Box average is the
  wrong shape (shaped noise rises steeply above 30 kHz): cutoff 45 kHz absolute, rebuilt per rate,
  ≥90 dB at output Nyquist proved by a DFT of the designed taps. DC blocker follows. Filter
  overshoots a step into all-ones, so full scale saturates at 8 388 607, not 8 388 608 (a 24-bit
  wire would read it as negative full scale).

## Sources

- **`LocalFiles` is the local provider**, replaced under `SourceId::local()` by the vault's
  `VaultFiles` where a vault is open (`vault.md`); other `MediaLocation`s refused by name unless a
  provider is registered (seam proved by a provider serving a track from memory in
  `crates/resonate-engine/tests/transport.rs`). A provider returns a `Read + Seek + Send + Sync`
  stream and says whether it is seekable; forward-only is spooled (see Decode).
- **A non-filesystem provider is waited on for `Sources::OPENED_WITHIN` only.** `Sources::open` asks
  `SourceId::local()` in line (a scan opens hundreds of thousands of files), any other provider on a
  `resonate-open` thread: no answer in time = `Error::OpenTookTooLong`; engine passes the row over,
  tag reader reads nothing. The abandoned call is not stopped, so a provider puts its own deadline
  on the network. **Reads likewise:** a `Deadlined` stream sends every read and seek to a
  `resonate-read` thread, waited on for `Sources::READ_WITHIN`; no answer in time =
  `io::ErrorKind::TimedOut`, stream stalled (every later call fails at once), engine fails the track
  instead of hanging.
- **Every track opens off the engine thread.** `Engine::start` hands the row to an `Opening`: a
  `resonate-track-open` thread runs the whole `Unwrapped::open`; engine parks on its answer beside
  its commands, publishing `Loading`. Commands are answered meanwhile (play/pause change whether it
  plays once landed, seek moves where it starts, stop/load/skip drop the opening). **The command
  that began an opening is answered by its landing:** `Engine::dispatch` defers the reply of a
  command that left a new opening in flight; `answer_for_the_opening` runs the outcome through
  `past_what_will_not_open` and the stranded-row handling; a deferred reply is answered `Ok` after
  `OPENING_ANSWERED_WITHIN` or when the opening is dropped (later failure = `Event::Failed`).
  `wait_for_the_graph` does not let go while a row is opening. A failed landing nothing waited on is
  passed over while playing (`Engine::fail`), else `Event::Failed`; thread stopping unanswered =
  `Error::OpenerStopped`.
- **A track that cannot seek decodes off the engine thread** (a still-arriving spool or replayed
  pipe waits for bytes with no deadline). `lending::Decoding` holds the decoder; `Track::next_block`
  *lends* it, with its block, to a `resonate-decode` worker: engine answers `Block::Awaited`, `fill`
  returns `Filled::WhenTheSourceAnswers`, engine takes the answer (`take_what_was_decoded`) next
  pass. A delivery set while the decoder is away is owed and applied as it comes home;
  `settle_the_spool` waits for it. Stop/load/skip drop track and worker. Seekable tracks, or ones
  whose worker could not start, decode in line. Worker stopping unanswered =
  `Error::DecoderStopped`.

## Transport

- **Bit-perfect wins at every track boundary; no gapless playback.** Track change drains the ring
  and reopens the stream: once dry, engine asks the graph to drain and waits for
  `StreamEvent::Drained`, so the boundary falls where the graph says the tail played out;
  `SinkStream::latency` on the wall clock is the fallback for a backend that never answers. A stream
  takes every instruction through one `StreamCommand`, keeping that handshake, `set_active` and
  `close` on one ordered channel to the loop thread.
- **Bluetooth headphones can be kept from cutting the start; off unless asked.** They power the
  radio down when nothing is sent. `BluetoothWake` (`bluetooth-wake`, `bluetooth-lead-ms`,
  `bluetooth-awake-s`) touches only a sink `SinkInfo::is_bluetooth` names. **A pause fades the ring
  out instead of standing the stream down:** consumer feeds real silence (zeros for PCM, marked word
  for DoP), link stays up (`Output::awake_since`) until `awake_for` passes (`let_the_link_rest`).
  **A stream starting on a slept link opens on silence:** `RingProducer::lead_in` pads frames before
  the ring is read. Slept = `Engine::sounded` against `LINK_NAPS_AFTER`, so a track change pays no
  lead.
- **Nothing the listener does cuts the waveform where it stands; it fades over `FADED_OVER`.**
  Deactivating/closing a stream or dropping the ring's contents would stop music mid-cycle (a
  click). The consumer fades (ring holds up to `buffer-ms` of rendered audio): `Fader` is shared by
  the ring's two ends; producer asks for silence or sound (`fade_out`, `fade_in`), consumer walks
  its `level` there a frame at a time, then feeds silence without consuming (`is_quiet`). Away from
  a fade nothing is touched. DoP ring never scaled (a multiplied marker is noise); goes quiet at
  once on its marked silence. **Pause** stands the stream down once the consumer is quiet
  (`finish_fading`), or after `quiet_within` where the graph never pulled. **Stop, track change,
  every rebind** retire the output (`Engine::retiring`): fades out, closes once quiet; `promote`
  opens no new stream until then (client holds one playback stream). **Seek in place** fades old
  audio out (`fade_into_the_discard`), new in. **Stream opened mid-track** opens
  `Entering::FadedIn`; at a track's start it opens whole, so a bit-perfect first frame is untouched.
  Only a stream the graph is pulling fades (`Output::is_sounding`, `PULLED_WITHIN`).
- **A volume or ReplayGain change is heard at once, on what the ring holds.** Consumer trims each
  frame by level-to-be-heard / level-rendered-at. `RingProducer::hear_at` stores the amplitude to be
  heard (atomic, f32 bits); `render_at(amplitude, ahead)` announces the frame (counted from the last
  discard) from which the chain renders at a new amplitude, via an SPSC queue of `RENDERED_SLOTS`
  carrying the discard epoch. `ahead` = rendered but not yet in the ring
  (`Output::frames_rendered_ahead_of_the_ring`: `Chain::frames_held_after_the_gain`, true-peak
  guard's delay, what a drain has not yet written): boundary lands on the exact frame, level never
  steps (`turning_a_resampled_track_down_retunes_the_stream_it_is_already_playing` checks frame to
  frame). Heard level walks to the wanted one over `FADED_OVER`; a frame rendered at the heard level
  is untouched (bit-perfect stays so), one rendered at nothing is left alone. Gain stage steps; the
  ring is the only ramp. A discard restarts the count at the last level announced. DoP ring never
  trimmed. A track that cannot seek and awaits its rebind is trimmed until its stream is rebuilt.
- **A mute is the ring's, not the chain's.** `Command::SetVolume(Volume::MUTE)` over a volume above
  it sets `Engine::muted`, leaves `EngineConfig::volume`, so the chain keeps rendering at the volume
  that comes back; the ring hears nothing (`Output::open` takes `muted` into the ring's first
  `Level`). PCM ring ramps to zeros; DoP ring feeds marked silence while still taking frames; stream
  keeps its plan. Unmuting is heard at once, bit-exact
  (`muting_is_silent_at_once_and_unmuting_is_heard_at_once_on_the_same_stream`).
  `PlayerState::volume` and the device's turn read `Engine::heard_volume`; a device's own reading
  clears the mute.
- **A seek does not reopen the stream.** rtrb cannot drop what the consumer has not read, so the
  ring carries a discard epoch: engine bumps it and stops writing, graph thread drains every slot on
  its next callback and acknowledges, engine refills; no acknowledgement within `DISCARD_TIMEOUT` =
  stalled, stream rebuilt. Graph gets the stream's own silence (off the plan, not a `memset`) until
  the ring is half full (`Output::primed`): one clean gap, no fault reported. Seek while paused
  reopens instead (a graph not calling `process` can never acknowledge the discard; reopening leaves
  the ring primed at the new position). A pause landing *after* an in-place seek is the same case
  caught late, read as `Unanswered::NothingPulling`, not a stall. Sink switch and renegotiation
  still reopen.
- **Leaving/returning to unity gain and switching the equaliser reshape the chain in place.**
  `retune` builds the wanted plan against the open stream. `OutputPlan::same_shape_as` holds: retune
  the chain. Else `OutputPlan::becomes_on_the_same_stream` (same `stream`, `packing`, `remix`,
  `resample`, convolution by `Arc<Impulse>` pointer, and under a resampler or convolver the same
  `restoration`): `build_chain` on the engine thread, swapped in under ring, stream and consumer, so
  nothing reaches the realtime thread. New chain renders at its own gain from its first frame and
  announces it (`Output::note_the_rendered_level`), the ring trimming what the old one rendered;
  decoded-but-unconverted samples retyped with `AudioBuffer::retype`. Dropping the equaliser first
  eases it out, holds the wanted plan in `Output::settles_into`, swaps once `Chain::is_ramping` says
  so (`finish_reshaping`, `eq.md`). A newer command drops what waited. **Resampler and convolver are
  carried across, not rebuilt** (a fresh resampler starts from silence and steps a steady level;
  flushing a convolver put its whole decay, up to `LONGEST_IMPULSE`, into the ring ahead of the
  music). Where `OutputPlan::carries_the_front_into` holds, `Chain::take_the_front` lifts the stages
  up to the last one `Processor::is_carried_across_a_reshape` names, the rest is flushed into the
  ring, `build_chain_after` builds the new stages behind; swap waits for the room the stages
  *behind* the front hold (`Chain::max_flush_frames_behind_the_front`) while the pump stops filling
  (`Output::waits_for_room_to_reshape`). A restoration switched under a resampler or convolver still
  rebinds; a volume or ReplayGain change never does (a converting plan always carries a gain stage).
- **A setting asking for the stream already open does not reopen it.** `Engine::keeps_its_stream`
  asks `plan_output` what the bound sink would open at now, weighed against the open plan's stream,
  packing and `ring_capacity`; `reopen_where_the_stream_moves` rebinds only where one moved. Graph
  rate not weighed (`clock.force-rate` pins the graph: switching it still reopens).
- **A renegotiation converges because the next plan asks for exactly what the graph answered.**
  `StreamEvent::FormatChanged` carries the settled spec; `downgrade` hands it to `plan_for` as
  target, written into `OutputPlan::stream` whole (writing the source's channels back made the two
  trade turns forever). `MAX_RENEGOTIATIONS` is the belt: a fourth different spec fails the track
  with `Error::Renegotiation`; reset when a track lands (`Engine::started`).
- **The run loop waits on every channel that can change its mind; the ring sets the timeout.**
  Wakers: command, sink announcement, survey answer, opening landing, worker's decoded block,
  stream-open answer, stream events. Timeout: half what the ring holds, floored at `SHORTEST_TICK`,
  capped at `PUBLISH_TICK` (`Published` refresh for a 60 Hz front end); `IDLE_TICK` with no stream
  or nothing playing or wanting more; transport at rest (not playing, fading, sleep-timed, graph
  lost, sinks stale, opening a track or stream, nothing filling, discarding, reshaping or ramping)
  parks `AT_REST_TICK`, every change that could move it arriving on a channel the `Select` wakes
  for. A disconnected receiver reports ready forever: `changes` dropped and `Output::deaf` set the
  first time one does.
- **`run` is two loops because a `Select` borrows its receivers.** The stream's receiver lives in
  the `Output` every `&mut self` method replaces, so a `Select` from `&self` cannot outlive a body
  that rebinds. `Heard` = that set lifted out (`Engine::heard` clones the `Receiver`s);
  `Heard::is_what`, comparing by `same_channel`, breaks the inner loop exactly when it changed.
  `unsafe_code = "forbid"` ruled out a self-referential field.
- **Sinks are enumerated mid-stream on their own thread.** `SinkChange` sets a flag; next tick hands
  it to `Surveying`, a `resonate-sinks` thread holding the backend's `Surveyor`, answering on a
  channel `Heard` parks on. Engine never waits on the graph. One survey in flight; a change
  announced meanwhile leaves the flag set and the landing answer asks again. No thread: enumerate in
  line only while no stream is open, under `SINK_TIMEOUT`.
- **A bind reads the published sink list; only an announcement refreshes it.** `select_sink` is on
  every track change, non-in-place seek, settings change and renegotiation; enumerating there would
  put a `SINK_TIMEOUT` before each against a daemon that stopped answering. `note_sink_changes` is
  read before the bind as well as on the tick; standing reasons to ask the graph again: stale flag,
  empty list, change stream gone; ask failing with a list still published: bind takes it. **No bind
  asks in line where the survey answers for the list** (`the_survey_answers_for_the_list`: survey
  thread + change stream). A list with no announced change since the survey last answered is
  current, empty included: a bind takes it, an empty one is `NoSink` at once. A stale list still
  holding devices is bound from as published; the survey's answer moves the stream via
  `follow_the_sink_it_would_choose`. A list known wrong (stale or survey out, and empty or with a
  row waiting for a device or the graph: `the_survey_will_answer_for_a_list_known_wrong`) is not
  bound from: `wait_for_the_survey` keeps the row at its frame in `unbound`, publishes `Loading`,
  marks it waiting for a device unless the graph is lost, command answers `Ok`; the answer binds via
  `bind_the_row_waiting_for_a_device` or `bind_the_row_the_graph_let_go`, each of which reads an
  `Ok` that left no output as the row waiting again (a change announced while the survey was out
  makes its answer known wrong too), never as bound
  (`a_device_announced_while_the_survey_is_out_still_binds_the_row_waiting_for_one`). `wait_for_the_graph` marks
  the list stale only while no survey is out, so the answer it waits on is not made stale by the
  wait. A failed first survey at startup leaves the list stale
  (`a_load_onto_a_list_with_no_devices_is_refused_without_asking_the_graph_again`,
  `a_load_while_the_survey_is_out_is_answered_at_once_and_bound_once_it_comes_back`).
- **Every stream opens off the engine thread.** `Backend::opener` hands out an `Opener` (PipeWire:
  the cloneable `Survey`, whose `open` waits up to `STREAM_OPENED_WITHIN` for the loop); `Streaming`
  is a `resonate-stream-open` worker taking one `Asked` at a time. `promote` hands it the request
  and the ring's consumer, transport stays `Loading`, commands answered meanwhile; `land_the_stream`
  takes the answer on a later pass (`Heard` wakes on it). Each `Output` is numbered (`Output::made`,
  from `outputs_made`): a stream landing for an output since retired or rebound is closed
  (`close_unwanted`), a failure for one only logged. **One open in flight, none asked while one is**
  (a stray's `Close` must reach the loop before the next `Open`; `Close` acts on whichever stream
  the client holds). `wait_for_the_graph` does not let go while an open is in flight, so an open
  refused `Disconnected` is read as the graph's. No worker: open runs in line. A test reading
  `opens` waits for it, not for the state the bind published (which comes first).
- **The engine decides which device the stream is on and follows the default itself.** Playback
  streams carry `node.dont-move`, so WirePlumber never relinks them (its `follow-default-target`
  moved the stream while `OutputStatus::sink` named the old device). Every sink-list refresh ends in
  `follow_the_sink_it_would_choose`: where `chosen_sink` (what `select_sink` binds through: named
  device, else default, else first) answers a device other than the output's, the row is bound again
  at its position. A desktop's per-application device picker cannot move the stream; the settings
  pane chooses.
- **A command never leaves the transport playing nothing.** Rebind or start retires the old output
  before binding, so a failure for the row's own reason (`Convert`, `Decode`) would leave no stream
  while the transport plays. `carried_on_past_a_stranded_row` sends an error naming a track
  (`Error::track`) that leaves the engine `is_stranded` through `fail` (reported, skipping on);
  command answers `Ok`. A device's error (`NoSink` to a `Load`) is answered as before.
- **A graph letting go of the ring is waited for once; the second time fails the track.**
  `RingProducer::is_abandoned` = consumer dropped (daemon restart). `watch_graph` closes the output,
  keeps the row with the heard position in `unbound`, publishes `Loading`; sink survey runs, landing
  answer binds the row again at that frame (`bind_the_row_the_graph_let_go`;
  `wait_for_the_graph_in_line` only where no survey thread could start). Waits `GRAPH_BACK_WITHIN`
  before failing the row; pause or stop ends the wait, another row does not: a track change or
  *Play* landing while the graph is away fails its bind with `Disconnected`, `LoopStopped` or a
  `Daemon` error, which `parked_for_a_device` reads, while `graph_lost` stands, as the graph's, not
  the row's. A bind refused `Disconnected` before `watch_graph` has seen the ring abandoned starts
  the wait itself (`graph_lost_again`, the one guard both share). Graph letting go again within that
  window raises `Error::LoopStopped`, not an endless loop (`graph_last_lost`). Asking the sinks
  first matters: a bind succeeds against the stale list and fails only when the stream opens. A
  disconnected *event* channel stays `Output::deaf` (a graph can stop reporting and go on pulling).
  A failing `backend.open` takes the whole `Output` with it (the consumer went into the call, cannot
  come back). A graph that lets go just as a row ends binds the row again at its end, and the next
  row opens a stream of its own, so that stream opens twice and the next row still plays from its
  first frame (`a_graph_that_lets_go_just_as_one_row_ends_plays_the_next_row_from_its_start`).
- **A device going is waited out, not billed to the row.** `Engine::fail` first hands its error to
  `parked_for_a_device`: `NoSink`, `SinkGone`, `StreamFailed` with a track open close the output,
  keep the row in `unbound` at the heard position, publish `Loading` (`Paused` where paused), mark
  the sink list stale, raise `Event::Waiting` not `Event::Failed`. Every sink-list answer then ends
  in `bind_the_row_waiting_for_a_device`, binding again where the row was heard: on the fallback
  where the desktop has one, else the next device to appear. A stream failing again within
  `GRAPH_BACK_WITHIN` of the last loss is the row's after all (`device_last_lost`). A `Load`
  answering `NoSink` to its caller is unchanged. With no output, `Engine::position` answers
  `unbound` where set, not the decoder's place (a buffer ahead of what was heard).
- **A fill is a slice, not a loop to a full ring.** `Engine::fill` decodes and writes for at most
  `FILLED_IN_ONE_GO`, answering `Filled::ForNow` where it stopped with room left; `pump` keeps that
  as `fill_owed`, `budget` answers no wait while it stands, so Pause or Stop is not held behind a
  deep ring's priming.
- **The PCM ring carries `u8`, not `f32`** (`f32` would convert every stream, breaking bit-accuracy
  for 32-bit sources). `ring_capacity` clamps the buffer setting between `MIN_RING_FRAMES` (or
  `BLOCKS_A_RING_HOLDS` of the widest block one pass writes, `Chain::max_process_frames`, where
  more) and `LARGEST_RING` bytes: a mistyped `buffer-ms` sizes a ring, not aborts. Block weighed
  because the converting `fill` writes only while the ring has room for one and `primed` waits for
  half the ring (a heavy upsample could else sit in `Buffering` with no error). A flush is not a
  block (a convolver's tail is handed on from `staged` a ring's room at a time, `drain`). **Ring
  depth is the engine's, never the graph's.** Every stream asks `LatencyRequest::Auto`: a quantum is
  a few milliseconds shared by every client on the device; a 100 ms-to-1 s ring written into it
  would drag the whole graph to PipeWire's largest quantum. Depth decides how long an equaliser
  change waits to be heard (volume/ReplayGain is trimmed in the ring) and how much an opening stream
  primes.
- **A track change does not start the transport; a command does.** `Engine::start` opens the row the
  queue is on at whatever the transport was doing: `Next`, `Previous` and removing the playing row
  leave a pause in place (what MPRIS says). `Load { autoplay }`, `Play`, `Command::JumpTo` and an
  `Insert` asked to be heard set `playing`. A track failing mid-play still advances into playback.
  **Nothing to hear sets nothing:** `Load` with no rows leaves `playing` clear; `Play` over a queue
  with no current row answers `QueueEmpty`. **A toggle weighs what is heard, not what was wanted.**
  A load or `Next` answered `NoSink` leaves `playing` set over a track with no stream and nothing
  waiting for a device (`Engine::is_stranded`): `TogglePlayPause` plays there, binding the row. A
  row waiting for a device is not stranded: the toggle pauses it.

- **A track change nothing will be heard through binds nothing.** `Engine::start` begins an
  `Opening` (`Unwrapped::open` on a `resonate-track-open` thread); landing paused, `Engine::started`
  records the frame in `Engine::unbound` and stops: a skip run costs one open a row, not a sink
  selection, ring, DSP chain, half-ring decode and `Backend::open`. Paused, every `rebind` records
  the frame instead; `Engine::play` spends it; a failed bind leaves it set (next `Play` retries).
  `unbound` also holds the row of a lost graph, missing device or unanswered sink survey, bound by
  their own paths. `PlayerState::output` stays `None` until bound.
## The queue

- **Positions index the play order, not the load order.** `PlayerState::queue_position`,
  `Command::JumpTo`, `Command::Insert`, `Command::Remove`'s `Span` = rows a queue pane draws (right
  under shuffle). `Queue::position` = load-order index (`None` while a queued row is heard),
  published as `PlayerState::loaded_position`.
- **An edit by position names its queue.** `Player::send_if_the_queue_is_still` /
  `request_if_the_queue_is_still` carry the `Queued::revision` the gesture saw
  (`Request::queue_seen`); `dispatch` answers `Error::QueueChanged` (`Cause::QueueMoved`, toast *The
  queue changed before that could happen*) if it moved, so another client's edit between draw and
  click moves or removes nothing
  (`an_edit_made_against_a_queue_another_client_changed_is_refused_and_touches_nothing`).
  `PlayerModel::send_by_position` (last polled revision) sends the queue's `Remove`, `Move`,
  `JumpTo`; `Insert`, the bus and MCP (rows named by id) are unguarded.
- **Ids.** A file no scan has seen takes its `TrackId` from `unclaimed_id` (down from `u64::MAX`
  past the queue's ids; `Unclaimed::beside` = that walk once per run). **A catalog file is queued
  under the catalog's id wherever a catalog exists:** `Host::held_as` answers the library row for
  location and span; `AddTrack`, `OpenUri`, the binary's `mpris::claimed_by` (files `resonate play`
  and the window get) and a resumed row (`Resumable::held`) ask before minting. **A row's id names
  that row alone:** `one_id_each` runs over all `Queue::load` / `Queue::insert` input, replacing an
  already-claimed id with a minted one (`Unclaimed::claim`). The queue mints, not callers (who mint
  against the last *published* queue): a duplicate id made `Tracks` publish one object path twice,
  so `RemoveTrack` and `GoTo` missed the second row.
- **Stamped by rows, not ids.** `stamp_of` hashes location and span; `Queue::rows_changed`,
  `RootView::play_playlist`, the binary's `play_queue` use it, so a renamed row still stamps as its
  playlist.
- **What was queued is apart from what plays.** `order` = loaded album or playlist (only list
  shuffle, a `RepeatMode::Queue` wrap and unshuffling touch); `next` = rows somebody asked to hear,
  in asking order. `after` = rows of `order` drawn ahead of `next`; `Seat` = row heard is
  `order[after - 1]`, `next[0]` or nothing; drawn = `order[..after] ++ next ++ order[after..]`.
  `next` is exhausted before `order` goes on; a heard or skipped-past `next` row *leaves the queue*,
  not joins the playlist. `PlayerState::queue_stamp` = `Queue::playing_from` (stamp of `order`
  alone, so `Library::playing_playlist` still badges the playlist while a queued row plays);
  `Queued::stamp` = every row, what `Keeping` weighs. *Previous* or a jump into the playlist keeps
  what waits. Load replaces `order`, keeps `next`; resumption carries it as `Resumption::next`.
- **Loading replaces what plays; queueing adds to what waits; landing is a `Placement`.**
  `Command::Insert`: `Next` (front of `next`, behind a queued row being heard), `Queued` (end of
  `next`), `At(row)` (drawn row; joins `next` where the row before it is the one heard or a queued
  one, else `order`). Nothing heard: all land in `order`. A row put into `order` takes the queue out
  of `Library::playing_playlist` by restamping (badge returns if dropped).
- **`Command::Insert` says whether to hear it.** `play: true` spends the row `Queue::insert`
  returns through `Engine::hear` (shared with `Command::JumpTo`), so they cannot disagree about
  starting a row (MPRIS `SetAsCurrent` is one command, no row computed off a stale published queue).
  `RootView::queue` asks wherever `PlayerState::current` is `None`.
- **Coming back is a command.** `Command::Resume` carries a `Resumption` (rows as playing, where
  each was loaded, row, frame, shuffled or not); sole caller of `Engine::start(at)` with other than
  `Frames::ZERO`. A frame past the row's end opens it at its start. Opens the row and stops
  (`unbound`): one open, one decoder seek, bind on first `Play`; not a seek (`Seeks` not stepped).
  Queue ids are not kept: `Queue::restore` claims a row's catalog id (`Resumable::held`, read from
  the catalog), mints the rest via `Unclaimed::beside`. Only bare `resonate` resumes; `resume` off
  discards what was kept, not merely stops adding.
- **Unshuffled order is apart from load order.** `Queue::unshuffled` = play order with shuffle off:
  a drag, sort or play-next while unshuffled edits it with `order`; toggling shuffle returns to it.
  Under shuffle a removal drops the row and a queued row lands after the row it follows. Not kept
  across runs.
- **Removing the playing row moves on as a skip.** `Queue::hand_on` = `advance` minus the
  repeat-track hold: queued row, else next row, else (`RepeatMode::Queue`) `wrap_to_the_start`,
  else `Engine::remove` stops with `QueueFinished`.
- **Returns in load order, plays in playing order.** `Resumption::rows` = load order,
  `Resumption::order` = play order over it. `resonate-core::plays_in`: an order naming each row once
  stands, else load order. `Queue::restore` writes the shuffle flag, not `set_shuffle` (would
  reshuffle). Volume and repeat are deliberately not kept (settings a run makes, not a place it
  reached).
- **Transport state is sampled like a play.** `Keeping` reads the `PlayerState` and published queue
  `Listening` reads; answers `Keep::Queue` (`Queued::stamp` moved: the whole run), `Keep::Order`
  (`Queued::revision` or shuffle moved, same rows) or `Keep::Place`, only if the row changed, the
  position moved `KEPT_EVERY`, or the transport rests elsewhere than the place kept. No queue
  position (queue played out) answers none; the last real place stands. Only the first allocates a
  `Resumption`. `Resumable`, `Resumption`, `Reordered` are `resonate-core`'s (engine builds,
  catalog stores).
- **A play is what was heard, sampled not hooked.** `Listening` takes a `PlayerState` beside the
  queue, sums frames the position advanced while playing (not for a sample whose
  `PlayerState::seeks` differs, nor a step over `A_SEEK` not counted as a seek), answers once per
  visit when half the track or `COUNTS_AS_HEARD` (whichever is shorter) has gone. A seek keeps the
  visit open (a seek back after the mark counts once) except one back from within `A_SEEK` of a
  known end (repeat wrap), which begins another; a position going backwards with no seek counted
  restarts the count. Sampler because the write is SQLite: `Library::track_played` would leave the
  run loop behind a scan holding the writer, so `RootView::count_a_play` and `resonate play`'s loop
  (`HEARD_SAMPLE`) run it off the engine thread; no catalog counts nothing.
- **A visit says how long it was heard, twice.** `Listening::heard`: `Counts(Played)` at the
  threshold, `Settles(Played)` once when a counted visit ends. A never-counted visit answers
  `Passes` as it ends if it reached `PASSES_AT_LEAST` (a flick through the queue is not listening;
  twenty seconds of forty songs is); `Library::passed` keeps it in `passes`, apart from plays
  (listening time counts it; a skip is no play). The pair joins by id, not location:
  `Library::track_played` answers the `Listen` it wrote, `Library::listened` spends it, so a failed
  write keeps nothing and a settle cannot hit the wrong row. A counted visit also answers `Hears`
  every `TOLD_EVERY` (a window closed mid-track loses at most that); `Listening::leaves` settles any
  open visit.
- **The sleep timer is the engine's (headless wants one).** `Command::SleepUntil(Option<Until>)`:
  `After`, `EndOfTrack`, `EndOfQueue`; `None` cancels; deadline on `Engine`. **Pauses** (a sleeper
  wakes where they were). End-of variants need only the edges in `skip`; the wrap would be missed,
  so `Queue::wraps_next` reads the two fields `advance` decides it from. **Fades out over the last
  `SLEEP_FADES_OVER`:** `sleep_is_due_in` answers what is left, `fade_toward_sleep` asks the ring
  for silence over exactly that. Cancel or push-back mid-fade restores the level; a seek or in-place
  restart lifts it (`lift_the_sleep_fade`, `sleep_lifted`). Outlives track change, seek, pause, new
  load (a timer on the listener). Delay capped at `LONGEST_SLEEP` so a huge value cannot overflow an
  `Instant`.
- **A skip under repeat-track repeats the queue, unless told otherwise.** `Command::Next` and
  `Command::Previous` going back a track are a person's skip (a track's end reaches `skip` as
  `natural`), so `skipped_by_hand` turns `RepeatMode::Track` into `RepeatMode::Queue`.
  `EngineConfig::skip_under_repeat` (`SkipUnderRepeat::KeepsRepeatingTheTrack`, key
  `skip-repeats-queue`) lives in the engine so media key, MPRIS `Next` and `resonate play`'s `n`
  obey it. A jump to a chosen row is no skip.
- **Previous restarts the song once past its opening, unless told otherwise.**
  `PreviousRestarts::starts_the_track_over` weighs the heard position (decoder less ring, chain and
  sink contents) against `PreviousRestarts::OPENING`. Past it under `RestartsTheTrack`,
  `Command::Previous` seeks to the start, queue untouched; within it, on a track no longer than it,
  with no track open, or under `AlwaysGoesBack`, it retreats. Unknown length: heard position alone.
  Restart is a seek (steps `Seeks`); retreat runs `start`. Default restarts; key
  `previous-restarts`, live via `Command::SetPreviousRestarts`.
- **A relative seek past the end moves on, as the bus's `Seek`.** `Command::SeekBy` landing at or
  past a known length = `Command::Next` (a held arrow reaches the next row, no `SeekOutOfRange`
  toast). Absolute `Command::Seek` past the end is still refused. A seekable stream of no declared
  length seeks normally (`Decoder::seek` lets the reader try).
- **Published position never steps back within a stretch of listening.** Sink latency is known only
  once a stream reports it, so a rebind or seek would publish a position a latency behind, which
  `Listening` read as a new visit. `heard_position` holds at or above the last; released by a landed
  seek, a track started, a stop.
- **Giving up is counted per run of failures, not per track opened.** `Engine::fail` stops the
  transport once more rows have failed than the queue holds; reset by a track played out, a stop, or
  a person skipping or choosing a row, not by a stream opening (a daemon taking every stream then
  dropping it would loop a repeating queue for ever).
- **A row that will not open is passed over, not stood on.** A deleted file, unmounted disc or
  playlist row `--tidy` would drop answers `Error::Decode` from `Unwrapped::open`;
  `land_the_opening` hands it to `Engine::fail` (report, skip) where the transport was meant to
  play, else emits `Event::Failed` and sits `Stopped` on the row. `past_what_will_not_open` does the
  same for every command meaning *play something*. `skip` asks whether the *queue* is on a row, not
  whether a track is open; `settle` hands a failed natural skip to `fail`, not `stop`.
  `Error::track` names the row in `Event::Failed`.
## What the engine publishes

- **An event is owed, not dropped, when nobody drains the channel.** `EVENT_SLOTS` fill while a
  front end is busy; the rest wait in `events_owed`, in order, for `hand_over_what_is_owed` at the
  end of every pass, so track changes cannot push out the `QueueFinished` headless `play` waits on.
  Past `EVENTS_OWED_AT_MOST` the oldest goes with a warning. `Underrun` is the one event let go,
  told at most every `UNDERRUNS_TOLD_EVERY` with missing frames summed.
- **One `Published` bundle, not a growing argument list.** `PlayerState`, `OutputSettings`,
  `StreamDigest`, queue, sink list, the visualiser's `Tapped` (plus its listening flag), each behind
  its own lock so a 60 Hz poll clones a pointer. The queue is a `Queued` (rows in play order, where
  each was loaded, revision) under *one* lock, so a reader cannot pair rows with another moment's
  order; queue and digest are written *before* the `PlayerState` advertising them.
  `PlayerState::seeks` = token `Engine::seek` steps where the seek landed and nowhere else (not a
  refused seek or track change), so a reader learns the position moved on purpose
  (`resonate-mpris` emits `Seeked` from it). `PlayerState::sleeping` carries what is *left*, so a
  front end keeps no clock. Queue republished only when `Queue::revision` moves.
- **A command is answered once the state answering for it is published.** `Engine::dispatch` keeps
  each reply as an `Answer`; `Engine::answer` sends them after `publish` at the end of the pass, so
  `Outcome::wait` means *the published state already says so*. `Reply`: `Player::send` (unwaited;
  refusal announced as `Event::CommandFailed`), `Player::request` (refusal in its `Outcome`, nothing
  announced), `Player::settle` (a `Landing` saying only that it was applied; refusal still
  *announced*). `resonate-mpris` settles every engine-moving call via `Shared::settle`, waiting
  `SETTLE`: zbus emits `PropertiesChanged` by calling the getter the moment a setter returns `Ok`,
  so a setter returning first announced the value just changed away from. A wait that runs out is a
  debug record, not a refusal.
- **A setting's *set* value comes from the engine, not the pane.** `OutputSettings` = the engine's
  whole configured output, a new `Arc` only when one moves, so the settings pane marks the chosen
  option from what is in force. Apart from `PlayerState` because the sink *name* is the choice and
  `OutputStatus::sink` the `SinkId` opened. `EngineConfig::sink` `None` = *follow the default*, so
  `Setting::Sink` carries `Option<NodeName>` and `config::clear` removes the key.
- **The inspector never rereads the playing file.** The engine samples `Decoder::last_packet`
  through a `ProfileBuilder` and publishes a `StreamDigest`; `resonate-ui` renders it without
  `resonate-codec`. Only the box layout reopens the source (the inspector's to draw), so `inspected`
  answers `None` where `probe_boxes` fails rather than failing a track that would have decoded.
  `Player::analyse` decodes the playing row whole on the window's background executor, only while
  the analysis pane is in front (`analysis.md`).
- **What is heard is tapped where written, read back by where the graph has got to.** `Tapping`
  sits on `Output` beside the ring; `Engine::fill`, `convert`, `drain` hand it exactly the frames
  `RingProducer` took (after equaliser, gain, dither; before the ring's trim) as f32 left and right
  at the sink's rate. A `Tap` = power-of-two ring of `AtomicU32` whose slots are a `OnceLock` laid
  by the first frame recorded while somebody listens (never opening the visualiser allocates
  none). No locks, the engine never waits on the window: a write claims its frames behind a
  release fence and publishes the count with release; a read acquires, reads, rereads the claim,
  silences any frame the writer may have gone round onto. The anchor (frames tapped less ring
  contents less `SinkStream::latency`) is fixed by `Engine::publish` every pass through a
  three-atomic seqlock; `Tap::around` runs on from it by the clock (at most `RUNS_AHEAD_AT_MOST`)
  and returns the frames *centred* on the one heard. `Player::listen_in` is the switch (unlistened,
  a block costs a load and a store; listening again marks `valid_from`, so what the ring held reads
  as silence). `Player::tap` answers `Tapped`: `Nothing`, `Samples`, `Markers` (DoP).
- **A profile holds at most `MAX_WINDOWS` points, whatever a packet claims to span.** Windows come
  from a packet's declared duration, so an absurd `dur` would ask for billions; once full, the open
  window is abandoned, bounding the `Vec` and the engine-thread walk, as in `probe_stream`.
- **A queued row nothing plays is read once, off the audio path.** `Player::media` answers from a
  bounded catalog filled by `READERS` (four) `resonate-tags-<n>` threads running `probe`, so a
  source that never answers holds back only the reader it holds
  (`a_source_that_does_not_answer_holds_back_no_more_than_the_reader_it_holds`); a miss requests the
  read and answers nothing (no blocking on a disc); `Player::media_revision` moves when one lands.
  Lets `resonate-ui` draw title, artist, length, cover for an unscanned file without
  `resonate-codec`; feeds `resonate-mpris` `GetTracksMetadata` under one deadline for the call.
  `Player::art` = the picture from the same entry; bounded apart (`ROWS_HELD`, `ART_BYTES_HELD`: a
  picture drops back to unasked while its tags stay), asked on own channels, read only when no name
  waits. What is read is a `Row` (location *and* the span the queue row names); both live in one
  `Entry` under the location, so a cover is asked once however many rows a sheet cuts and eviction
  takes a file with all its rows.
- **Seeing a row unasked and claiming it is one operation under one lock.** `Claim` is what the
  shelf answers, so the 60 Hz poll and bus thread cannot both enqueue a location. The guard never
  leaves `Shelf` (a `match` scrutinee holds its temporary for the whole match, so a `Catalog`
  locking the shelf itself would deadlock on the arm that asks): `Shelf::claim_tags` takes and drops
  the lock in a call of its own.
- **What may be asked at once is bounded; a turned-away row is asked again.** Request channels hold
  `ROWS_ASKED` names, `PICTURES_ASKED` pictures, `LOOKS_ASKED` looks; a non-fitting ask answers
  `Sent::Backlogged` and the claim is *released* to `Unasked`, so the next poll asks for what is
  still on screen, not every row a scroll passed.
- **Catalog ordered by use, not searched for the stalest.** `Held` keeps a `BTreeMap` from use clock
  to row (`order`) and a second of rows with a picture (`pictures`), both re-keyed on each look, so
  eviction and `trim_pictures` take a step from the front.
## DSP

- **Chain: remix, lossy restoration, resample, convolver, equaliser, gain, true-peak guard, dither**
  (equaliser: `eq.md`). Equaliser after the resampler (a biquad's shape warps with its design rate;
  source-rate design sounds different per track), before gain (volume is the last word, should
  attenuate a boost). Convolver before it (both linear): part of the front a reshape carries. An
  equaliser in force makes the plan `Converted`; a DoP-packed stream refuses it.
- **Resampler levels: one filter design at four lengths; High is default.** `Quality::params` is the
  whole difference (`half_taps`, phases, cutoff, Kaiser β); settings pane prints it on hover (hence
  `SincParams` re-exported by `resonate-engine`). `VeryHigh` beats SoX `rate -v` on paper; only an
  f64 carrier shows it (`very_high_beats_sox_very_high_on_paper`,
  `each_quality_meets_its_alias_rejection_floor`).
- **Resampler phase is its own setting; a shaped phase is designed once a run.** `FilterPhase`
  `Linear`/`Intermediate`/`Minimum` (SoX `-L -I -M`) on `ResamplerConfig`, `EngineConfig`,
  `OutputSettings`; `Command::SetFilterPhase`, `filter-phase` key, `--filter-phase`. `phase.rs`: the
  quality's linear kernel (sampled at `SAMPLED_PER_TAP`, transformed at `CEPSTRUM_PADDING` times its
  length) folded through the real cepstrum to minimum phase (`Intermediate` averages with the linear
  delay). Intermediate is longer than the linear kernel: kept 1.25 half-widths before its peak to
  two after. `DESIGNED` keeps designs for the run, keyed by `SincParams` and phase (same prototype
  whatever the ratio).
- **Reach is two numbers (shaped kernels are asymmetric).** `Reach { behind, ahead }`: source frames
  of history an instant weighs / frames it waits for. History primed with `behind` zeros, flushed
  with `ahead` zeros; latency `ahead` source frames (`ahead * ratio` output); a track keeps its
  length whatever the phase (`a_track_keeps_its_length_whatever_the_phase`).
- **Kernel polyphase, history planar, read position exactly rational.** Position = `whole` sample
  index + `phase` numerator over phase count, integer-stepped. Count =
  `SampleRate::ratio_to(output).numer` = output rate / gcd (1 for equal rates or integer downsample;
  160 for 44.1 to 48 kHz); an f64 accumulator stepping `1 / ratio` smeared those 160 into over a
  thousand. `Tabulated` builds all phases once in `Resampler::new`, each divided by its own sum
  (every phase passes DC at unity). Last table kept in an `Arc` keyed by rates, `SincParams`, phase
  (only up to `KEPT_TABLE_BYTES_AT_MOST`). `TABULATED_WEIGHT_BYTES_AT_MOST` is sized so every pair
  of common rates within 32:1 tabulates at every level
  (`every_rate_pair_tabulates_its_phases_at_every_quality`); a rate no device offers uses `Kernel`
  (each weight off a four-point Lagrange stencil). History: one f64 plane per channel; a frame's
  channels share one pass over the weights, in pairs, `ACCUMULATORS` independent lanes. Planes,
  lanes, table aligned to `VECTOR_BYTES`, rows zero-padded (`LANES_PER_VECTOR`): no scalar tail.
  Lanes pass through `lanes` before summing (summed in registers, LLVM's SLP vectoriser paired the
  two channels instead of packing each one's lanes).
- **Dither: flat or shaped path, f64; the plan's curve picks.** `Dither::prepare` resolves
  `NoiseShaping::at` the output rate once into `applied`; `None` requantises with no error history,
  allocates none. Worked in target-grid steps in f64 (exact for every grid to 32 bits; f32 rounded
  the noise near full scale, biasing the result): 32-bit targets dither like any other,
  `Dither::new` cannot fail (an integer source an S32 word holds whole is still `Repacked`). Output
  clamps to `[-1, 1 - step]`; fed-back error is taken before the clamp; one not finite or past what
  the dither can make (`DitherKind::largest_error_steps`) feeds back as nothing (one NaN/wild sample
  would ride the feedback all track). Shaped path: one function generic over tap count, each
  channel's errors held twice in a ring twice the tap count (recent errors = one contiguous window);
  the match is on the enum, so a new `NoiseShaping` curve must say which path it takes.
- **Lipshitz only at its design rates; Threshold designed at the rate it runs at.** Lipshitz =
  E-weighted 44.1 kHz fit: `NoiseShaping::at` resolves it to flat except at 44.1 and 48 kHz.
  `Threshold`: `Dither::prepare` designs an error-feedback filter for the output rate from
  Terhardt's (1979) threshold in quiet (held at its 1 kHz value below 1 kHz, clamped to
  `THRESHOLD_RANGE_DB`): order-12 prediction-error filter by Levinson–Durbin, noise transfer
  function `A(z)` itself, minimum phase by construction. `plan_for` stores the surviving curve in
  `OutputPlan::shaping`; `resonate explain` prints it (a Lipshitz fallback to flat is visible).
  `EngineConfig` defaults to `Threshold`: a 16-bit device at any rate, or 24-bit behind volume below
  full, gets shaped dither out of the box.
- **Digital silence held a moment is handed on as digital silence.** Dither decorrelates error of a
  signal the grid cannot hold; exact zeros have none, and dithered, a gap reached a 16-bit device as
  shaped hiss a DAC's silence detection never sees. Once every channel has read zero (or under
  `SILENCE_FLOOR`, where an equaliser tail rings) for `SILENT_FOR_BEFORE_MUTING_SECONDS`, the stage
  writes zeros and clears its error history; the first non-zero frame dithers again from clean
  history. A fade is never silence.
- **`OutputMode` says what the plan does to the signal; a shorter word is not a resample.** Decided
  in `plan_for`, drawn as the chip on playback bar and inspector: `BitPerfect` (stream = source's
  triple), `Repacked` (target holds every source value losslessly, container wider), `Dithered`
  (rate and channel map the source's, word shorter, dither covering it), `Converted` for all else
  (incl. narrowing with dither off). Stream request `no_convert` = `mode != Converted`. Order is
  load-bearing: `dithers` resolves *before* the mode (a narrowing with dither off is a truncation,
  must not carry a name saying it was covered).
- **Channel layout is negotiated like rate and format; one matrix is the whole downmix.**
  `plan_output` asks `SinkInfo::best_spec_for` for rate, format *and* layout together: the plan
  never names a triple the sink did not advertise
  (`every_spec_the_chooser_names_is_one_the_sink_advertised`, `resonate-pipewire/src/sink.rs`, over
  the cross product). Fit is lexicographic: rate, channels, format
  (`keeping_the_channels_outranks_keeping_the_depth`). `Remix` (`resonate-dsp`) runs first, so
  resampler and dither cost the sink's channels. Matrix from `ChannelPosition`: channel the target
  has: copied at unity (`UNITY`); lacking: folded into nearest neighbours at −3 dB (`MINUS_3_DB`);
  LFE dropped, not folded; unfed target channel silent. Routing by position, never index (index
  one-for-one only where a layout's positions are unknown). Each row scaled to gains summing to at
  most one: no downmix clips a full-scale source.
- **The chain is f64 from decoded word to sink word.** `Processor::process`/`flush` take `f64`
  slices; the engine widens the decoder's output exactly into `Output::widened`
  (`SampleData::widen_into`), narrows once, into the sink's word, in `Output::stage` (an f32
  carrier's 24-bit mantissa lost a 32-bit integer source's low byte). `build_chain` still names the
  carrier `SampleFormat::F32` (spec = negotiation vocabulary, no wider float; stages read only rate
  and channels).
- **Lossy restoration: spectral stage for MP3, AAC, Vorbis alone; off unless asked.** `Restoration`
  `Off`/`Repair`/`Extend` (`restore-lossy` key; `LossySources` settings group has the `EXPERIMENTAL`
  badge). `Restore` pushed after remix, before the resampler, at the source rate where the encoder
  cut. `Decoded` carries the source's `Tuning` (engine `tuning_of` names the lossy codecs, not
  `is_lossless`, which calls `Unknown` lossy too) and the lowpass wall, off `TrackHints` (the
  study's), else found as the music plays. Lossless sources never get it; one that does makes the
  plan `Converted`. Weighted overlap-add holding back its first `frame − hop` frames, handed on in
  its flush (track keeps length). **Repair** lifts the droop under the wall, fills holes with noise;
  **Extend** also rebuilds wall to 97 % of Nyquist from the same width below, never louder than 3 dB
  under its source. Codec curves live in `Tuning`, applied in the spectrum, not given to the
  equaliser. Latency is flushed before a swap, as the guard's is.
- **A `Chain` carries a channel width per stage** (`Remix` changes frame width mid-chain): the
  builder records `output_spec(spec).channel_count()` per stage, sizes scratch by the widest
  *sample* count. **One flush drains a chain; a flush with less room is refused:** the builder sums
  what every stage can still hand on into `Chain::max_flush_frames`; `Chain::flush` answers
  `Error::OutputTooSmall` for a shorter destination. One flush is the whole tail on purpose (a
  resampler pads half its filter, keeping a track exactly its length); `Engine::flush` sizes the
  carrier to `max_output_frames`.
- **Clip prevention attenuates where the peak is known; only an unseen over is ridden down.** A
  ReplayGain boost is capped where the declared peak would reach full scale. `AppliedGain`
  (`resonate-core`) pairs gain with the selected peak and owns the arithmetic (DSP stage, `resonate
  info`, inspector cannot disagree). `AppliedGain::heeding` folds a measured true peak beside the
  declared one, keeping the larger: a track passing full scale between samples is turned down by
  exactly that even with ReplayGain off (`true-peak` off leaves the declared peak). Per-sample clamp
  survives only where a *boost* has no peak and no guard follows; with `true-peak` on, `gain_of`
  sets `GainConfig::guarded_after` and overs reach the guard whole. At unity or below the guard
  answers, so `GainConfig::limits_every_sample` reads amplitude as well as peak. A boost capped at
  exactly unity changes no sample: `AppliedGain::adjusts` false, track plays bit-perfect.
- **What the catalog studied reaches the player through `Hinting`, beside `StandIn`, not gated on
  the vault.** `resonate-core::TrackHints` (true peak, lowpass wall, measured gain) is core
  vocabulary (codec `Sources`, library, engine all need it). `Library::hinting` answers from
  `track_studies` by the row's path and span; `Unwrapped::open` asks once, folds it into the gain in
  `levelled`, which `re_level` and `Command::SetTruePeak` rerun.
- **A track nobody studied is measured where its peak would matter.** `true-peak` on, sample-packed
  (non-DoP) source, gain in force has no peak (tag or study) and asks for a boost or source is
  float: `measure::Measuring` decodes the row whole on a `resonate-peak` thread through a
  `TruePeakMeter`; each run-loop pass asks if it landed. `Track::heed_what_was_measured` puts the
  peak into the hints, `levelled` reruns, `retune` reshapes the chain in place. Dropping the track
  drops the `Measuring`. `Track::peak` is a `Peak` (`Unasked`, `Measuring`, `Unmeasurable`) so a row
  that will not decode to its end is not decoded again per settings change. Not kept; the lookup's
  study is what the catalog keeps.
- **True-peak guard: a lookahead gain, not a clipper; touches nothing under the ceiling.**
  `TruePeak` (`resonate-dsp`) pushed after gain, before dither, wherever `true-peak` is on and the
  plan converts, never on bit-perfect or repacked streams. Reads each frame at 8x rate; where a
  phase passes −0.1 dBTP it asks for the gain bringing it under; the smallest ask across the
  lookahead is averaged over the same span, so gain has fallen by the peak's frame. A stream never
  passing the ceiling is multiplied by exactly one and comes out bit for bit, only later: audio
  waits `lookahead + 48` (`REACH`) frames, held and handed on in its flush (track keeps length);
  `Engine::swap_chain` first flushes the running chain's held tail into the ring
  (`Output::hand_on_what_the_chain_holds`). Interpolator is 96 taps: every fractional phase must
  pass a tone near Nyquist at its own level inside the 0.1 dB between ceiling and full scale; the
  study's `TruePeakMeter` uses the same one. Interpolation skipped where nothing can be over (no
  phase exceeds loudest sample times the largest sum of absolute weights, about 3.0: `quiet_below`,
  `loudest_that_could_pass`); limiting is constant time.
- **Pre-amp and an untagged track's gain are part of what ReplayGain asks for; clip prevention
  weighs them.** `Levelling` = a `Trim` each (quantised millibels, so `OutputSettings` keeps `Eq`);
  `resolve_replay_gain` folds them into `AppliedGain`: tagged gain raised by pre-amp, keeps its
  tagged peak; no gain declared takes `untagged` with no peak (the one case the per-sample limiter
  is for). Keys `replay-gain-pre-amp`, `replay-gain-untagged`. **A studied track declaring no gain
  is levelled by what was measured:** `TrackHints::measured` is a `MeasuredGain` (ReplayGain 2.0's
  −18 LUFS less integrated loudness; album: duration-weighted energy mean of every track's loudness,
  only where every album track is studied). `engine::tagged_or_measured` uses it only where tags
  carry neither gain (a tagged file is never second-guessed); it gives a delivered row a gain (vault
  stripped its tags).
- **The plan alone judges a gain stage; a converting plan always carries one; the engine picks its
  fill branch from the plan.** `gain_config` answers `None` at full volume under unity gain, but
  `plan_for` asks for a stage anyway wherever the plan resamples, remixes, equalises, convolves or
  restores (not bit-perfect whatever the gain; a stage already there makes every volume/ReplayGain
  change a retune, not a shape change). Only a plan with no other stage omits it; `dop_survives`
  still reads `gain_config` (DoP refused only where a gain would change samples).
  `GainStage::is_transparent` is false, so the builder keeps every pushed gain stage. `Engine::fill`
  and `Engine::flush` pick byte copy or conversion from `OutputPlan::is_transparent`, the reading
  `delivery()` asks the decoder's format from.
- **Every float reaching an integer word is rounded to nearest, ties to even, saturated (NaN to
  zero); one set of core conversions is the whole of how.** `SampleData::write_f64` is what
  `Output::stage` narrows the carrier through to the ring; `write_f32` is its narrow twin
  (`convert_into`, `retype`, DSD decimator). A narrowing nothing dithers (dither off, or float
  source at its own rate onto an integer word) is `OutputPlan::rounds`: non-transparent plan, empty
  chain, so samples reach `write_f64` rather than symphonia's flooring shift or a truncating cast
  (dead band a step wide around zero). Rounding adds a constant making IEEE round the sum to a
  whole, half to even, instead of `round_ties_even` (`x86-64` baseline has no `roundps`; the
  addition vectorises on SSE2).

## The sink

- **A sink's formats are re-read when the node says they moved.** A port switch or EDID re-read can
  change a node's formats while the node stays; its `info` event is watched for `PARAMS`, the
  `EnumFormat` entry's `SERIAL` flag flipping per change. `SinkRecord::formats_moved` compares it
  with the last seen, drops every index held, asks for `EnumFormat` again; `SinkChange::Reformatted`
  has the engine survey the graph afresh, issued after the enumeration.
- **A running device says its own rate.** `SinkInfo::current_rate` = the sink node's `Format` param
  rate, else `clock.force-rate`, else `clock.rate` (settings metadata). A node has a `Format` only
  while running (suspended: none); its `SERIAL` flag is watched as `EnumFormat`'s is
  (`SinkRecord::running_moved` drops the rate, asks again, announces `Reformatted`);
  `format::running_rate` reads the rate whatever sample format the device runs in, planar included
  (`a_running_device_reports_its_own_rate_and_the_graphs_once_it_stops`,
  `a_running_device_names_its_rate_whatever_sample_format_it_runs_in`). A Profiler global would say
  the same of a driver; not subscribed.
- **A global leaving the registry takes its proxy with it, whatever it was told first** (a card
  unplugged before its routes were enumerated keeps no proxy/listener until reconnect).
- **A stream's listener goes before its stream.** `pw_stream_destroy` frees the hook list the
  listener's `spa_hook_remove` writes into, so an open playback or capture stream is an `Opened`,
  whose `Drop` takes the listener first; never a tuple, whose first field (the stream) drops first.
- **The client outlives its daemon.** A connection's everything (core, registry, listeners, bound
  proxies) is one `Graph`, made by `Reaching`; the loop keeps main loop and context for the
  process's life and connects a core through them as often as needed. Core `error` event with broken
  pipe, reset, abort or missing connection = daemon gone: sends `Request::Lost` through the loop's
  own channel (a connection cannot be torn down in its own callback). `Lost` drops streams and
  graph, answers every pending `Sync` by dropping it, empties `Discovered`, announces each known
  sink removed; a thread sends `Request::Reconnect` a second later, repeating until a core connects,
  **the first connect being one more try**. **A daemon silent without closing is lost too:** a loop
  timer sends `Request::Heartbeat` every `HEARTBEAT_EVERY` (a core `sync`, kept as a `Beat`); a beat
  unanswered after `DAEMON_ANSWERS_WITHIN` goes through `lose_the_graph`, the broken-pipe path;
  reconnection retries until the daemon answers
  (`a_client_whose_daemon_stops_answering_takes_it_as_gone_and_finds_it_again`, `SIGSTOP`s its
  hosted daemon). **Nothing inside the request callback sends on the request channel** (pipewire-rs
  runs the callback under the channel's lock; a send from there never returns). With no graph, a
  `Sync`, open or capture answers `Error::Disconnected` at once, not `LoopStopped` after a timeout
  (`PipeWire::unanswered`). `tests/reconnect.rs` reruns its binary under `PIPEWIRE_RUNTIME_DIR` with
  its own `pipewire`. **A recording survives the restart:** `resonate-listen`'s `Capture` reads its
  disconnected events channel as lost, asks the same client for the same capture every
  `LOOKS_EVERY`, recording into the same `Recording` from where it had filled.
- **A metadata key cleared reads as unset.** `Discovered::heard` reads `settings` and `default`
  metadata keyed by speaker (`HeldIn`): a key with no value clears what it held; a key of nothing
  clears every key that object carries and no other's (as `pw-metadata -d`). The default sink
  announces `SinkChange::DefaultChanged` only where the name it reads moved. **Only the core
  subject's keys are read** (subject 0, `CORE_ID`): `default` metadata also carries per-node keys
  (WirePlumber's `target.object`), cleared with a key of nothing when the node leaves; reading that
  as every key cleared wiped the default sink and rebound the engine to the first sink. **A metadata
  object leaving the registry is the same clear:** `Metadatas` keeps each proxy with the `HeldIn` it
  was bound for; `global_remove` drops it and reads a key of nothing for that object, so a
  WirePlumber restart leaves no value from a gone object
  (`a_metadata_object_that_leaves_takes_the_values_it_held_with_it`).
- **The chosen sink is named, not numbered.** `EngineConfig::sink` and `Command::SetSink` carry a
  `NodeName`, matched by `select_sink` on every stream open (PipeWire ids are per object; a
  replugged device gets a new one). `OutputStatus::sink` stays a `SinkId` (what was opened). An
  unanswered name warns and falls back to the default until the device turns up.
- **Hardware-ness is a question about the `Device` above the node.** A `Node` global carries
  `device.id`, not `device.api`. `Discovered::driven` = device ids whose global names a driver;
  `SinkRecord::device` = the id the node points at; `Discovered::is_hardware` answers for the pair
  at snapshot time, so the globals may arrive in either order (`resonate sinks` *DRIVEN BY*).
- **The physical port is the `Device`'s to answer; the seat is the join.** A card's `Route` params
  name its ports; a sink node's `card.profile.device` names the seat on the card;
  `DevicePorts::serving` is the pair. Seat read off the node's `info` only where the change mask
  says `PROPS` (a volume change raises `info` with no props, which must not read as *no seat*).
  `Route` = the port the card is *switched to*, outranks the rest (`Held::Current`); `EnumRoute` =
  every port offered (`Held::Offered`), the only place an unplugged port appears. `Plugged`: `Yes`,
  `No`, `Unsaid` (no jack detection).
- **The card says which output it is switched to, and the port whose volume it is.** `Device`
  subscribes to `Profile` beside its two route params; `parse_profile` reads the current one into
  `Discovered::profiles`; `CardProfile` = its `ProfileIndex` and description alone (no switching
  offered); a moved profile announces `SinkChange::Switched`. `HardwareVolume` from
  `route.hw-volume` in `SPA_PARAM_ROUTE_info`: `Yes`, `No`, `Unsaid`, never drawn as `No`.
- **A device turning its own volume can take the slider; the stream then stays bit-perfect at any
  volume.** `device-volume` (off by default) = `EngineConfig::device_volume`,
  `Command::SetDeviceVolume`, `OutputSettings::device_volume`. `Attenuator::of` weighs it against
  `SinkInfo::turns_its_own_volume` (route says `route.hw-volume` is `Yes`, never `Unsaid`);
  `Attenuator::leaves` = the volume the stream still applies: `Volume::MAX` where the device takes
  it (plan keeps no gain stage for it; a ReplayGain adjustment stays the stream's). The device's
  volume is its route: `PipeWire::set_device_volume` sets `Route` on the `Device`, loudest channel
  at `Volume::to_gain`, others scaled by the same factor, `save`d (what pipewire-pulse writes for a
  desktop slider; node `Props` would be the adapter's software volume), **keeping balance and
  mute**; `Command::SetDeviceMute` reaches `PipeWire::set_device_mute`. **The slider starts where
  the device is** (`Volume::heard_at` of the route's reading; handing over cannot jump headphones to
  full) and follows a device turned from the desktop: a `Route` is not re-sent when volume moves, so
  the device's `info` saying `PARAMS` changed enumerates again, `SinkChange::Turned` names the
  sinks, the survey ends in `follow_the_devices_volume`. The engine's own sends echo back the same
  way: `Engine::turned` remembers the last `TURNS_REMEMBERED` gains, a reading within
  `ONE_LEVEL_WITHIN` of any is an echo. **Desktop mute is kept apart from level:**
  `OutputStatus::device_muted` = `Attenuator::hears_the_mute_of` the bound sink; `DevicePorts::keep`
  counts a moved mute as a turn; the window's mute mark sends `SetDeviceMute(false)`.
- **Stream latency is counted in the stream's frames, converted where the graph tick rate is
  known.** `pw_time.delay` is in `pw_time.rate` (graph clock), not the stream's. The `process`
  callback holds the only consistent delay/rate snapshot: builds a `GraphTime`, publishes
  `downstream(spec.rate)` as one `AtomicU64` of stream frames (two atomics would tear; converting
  later takes a lock the RT thread may not). `SinkStream::latency` is `Frames`.
  `GraphTime::buffered` is already at stream rate: added, not converted; so is `GraphTime::queued`:
  buffers filled and not yet taken, `pw_time.queued_buffers` times the frames the last cycle filled
  (`Cycle::note_filled`, in packed frames where packed); `pw_time.queued` stays zero (pipewire-rs
  sets no `pw_buffer.size`) (`each_queued_buffer_is_reckoned_to_hold_what_the_last_cycle_filled`).
- **Bit-perfect is what the graph runs at, read every cycle, not what the stream opened at.**
  Another client can hold the graph at a second rate after the stream opened; the graph then
  converts the stream however the plan reads. `StreamClock` is what the callback writes each cycle
  (latency and, where a tick is one frame, `GraphTime::graph_rate`; both atomics), `SinkStream`
  reads; `poll_stream` copies the rate onto `OutputStatus::graph_rate`; `OutputStatus::hears`
  demotes the published `mode` to `Converted` wherever `converted_by_the_graph` (graph rate known
  and not the negotiated one), the plan's own mode returning once it matches. The plan is untouched:
  `no_convert` and every reshape still read `OutputPlan::mode`. Inspector: *→ 48 kHz by the graph*,
  *graph runs at* (`a_stream_the_graph_runs_at_another_rate_is_not_called_bit_perfect`).
- **The callback fills the quantum the graph asked for, not the buffer handed over.**
  `pw_buffer.requested` = quantum in frames; `Cycle::asked_for` turns it into bytes, clamped to the
  pool's room, falling back to all of it where the graph names nothing (also where the packed DoP
  path lands). Filling the whole pool buffer drained the ring many quanta at a time and declared an
  underrun against that much want.
- **Silence about a capability is not a refusal.** A node answering `EnumFormat` with no channels
  property, or no formats, is believed about what it said and left alone about the rest: candidates
  fall back to the source's own layout, or `allowed_rates` crossed with the source's format.
  `SinkInfo::supports` stays strict (exact format at exact rate; DoP path, `resonate explain`,
  settings pane read it) but a format entry naming no channels takes any layout
  (`SinkFormats::takes`).
- **A sink's advertised formats and the graph's `allowed_rates` are separate fields.** A device
  advertising 192 kHz is irrelevant if the daemon will not switch the graph to it; conflating them
  makes the bit-perfect claim unfalsifiable.
- **Twenty-four bits: one depth, two words; the wire word is the graph boundary's alone.**
  `SampleFormat::S24` = sign-extended low 24 bits of an `i32`, four bytes everywhere in the
  workspace; a device may want three. `format::WireWord` (`resonate-pipewire`) is the difference:
  `sample_format` reads `S24LE` and `S24_32LE` as one depth, `spa_format` takes the word as a second
  argument. `SinkFormats::words` is a `Words` (`resonate sinks`, `explain` say which were named).
  `build_stream` offers both as two `EnumFormat` params, packed first (a 24-bit DAC's default is
  usually packed; a conversion avoided is the bit-perfect point). The graph's choice returns via
  `param_changed`, stored on an `AtomicBool` the callback reads, sent as
  `StreamEvent::FormatChanged`, kept on `OutputStatus::words` (inspector: *on the wire*).
- **Packing is in the graph's own buffer, forward, allocation-free.** `process::pack` reads the low
  three bytes of the word at `4i`, writes them at `3i`; `3i + 3 <= 4i + 3`, so the write never
  reaches an unread word, no scratch. The ring still fills at the padded stride and `Silence::write`
  still lays DoP markers as four-byte words; packing keeps bytes 0 to 2 and the marker is byte 2, so
  a DoP carrier survives. Without it a 24-bit master reaching a device advertising `S24LE` and
  `S16LE` alone was dithered to 16 bits.

## Fixtures

- **Capture and playback tests on the desktop graph skip with no daemon.** `PipeWire::start` starts
  a client before a daemon exists, so `resonate-listen`'s `tests/capture.rs` and
  `resonate-pipewire`'s `tests/stream.rs` read `Error::Disconnected` from the first sink discovery
  as a skip. Other discovery failures fail; reconnect tests host their own daemon.
- **A test asserting which row plays pauses the transport first.** The fake graph is pulled by the
  test thread alone, but a decode failing on a loaded machine reaches `Engine::fail`, which skips.
  `stand_still` is the pause; queue-order tests take it before reading a row and again after any
  command starting the transport.
- **Real-file fixtures are built by ffmpeg at test time; the suite skips without it.**
  `crates/resonate-codec/tests/encoded.rs` also builds a rip through `flac`/`metaflac` (block list
  asserted; gated on those tools), MP3s through ffmpeg's `libmp3lame`, and writes one CAF itself
  (ffmpeg's muxer refuses AAC): ADTS packets in `desc`, `pakt`, `data` chunks as `afconvert` would.
- **`RESONATE_REAL_FIXTURES` points the suite at a folder of real files.** Every file must probe,
  name a known container and codec, report a duration and decode its first block. Unset: skip.
- **Transport and bus tests drive a real `Player` thread and poll for the expected state**: a state
  that never arrives costs the full patience; the failure names the transport it watched.

