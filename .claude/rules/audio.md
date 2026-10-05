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

- **A source's depth is read from the container, never guessed from the decoder's buffer.** symphonia
  leaves `bits_per_sample` unset for ALAC, so `container.rs` takes the depth from the magic cookie;
  otherwise a 24-bit ALAC claims 32 bits and `plan_output` asks the sink for a format the file never
  had. `declared_bits` asks every container that answers, nearest the codec first (coded width, a
  WAV's `wValidBitsPerSample`, FLAC's `STREAMINFO`, Matroska's `BitDepth`, then `bits_per_sample`),
  because the depth a file was made at is what the inspector prints and padding is a lie about the
  master.
- **A layout is named only where the container places every channel where that layout does.**
  `positioned_layout` weighs symphonia's speaker mask against the named layouts; anything else (a 6.0,
  an LCRS) is `ChannelLayout::Discrete` rather than whichever layout shares its count, since a downmix
  reads a 5.1's fourth channel as the LFE and drops it. One and two channels are mono and stereo
  wherever placed; an unplaced set of more is `Discrete`.
- **A tag is read under the naming its writer used, not only its container spec's.** symphonia maps a
  raw key through the container's vocabulary, so ffmpeg's Matroska (`ALBUM`, `ALBUM_ARTIST`, `DATE`)
  reached the `TagSet` through nothing. `tags::Naming` decides from the key set which vocabulary is in
  play: one name the spec never defines (`VORBIS_COMMENT_ONLY`, holding only names no container spec
  also defines, so `DATE`, `BPM`, `ISRC`, `LABEL` and similar are absent) means Vorbis comments. What a
  reader never exposes is taken off the source first (`riff.rs` for a WAV's `INFO` and `id3 ` chunk,
  `matroska.rs` for the segment `Title`, which fills a track only where the container holds one audio
  track), `Prescan` being the one bundle carrying both. A WAV's `id3 ` revision is appended *last*, so it
  outranks `INFO` and a leading tag. `IDIT` and `DTIM` are excluded from `INFO` reading: they date the
  digitisation, not the release.
- **What the `TagSet` holds is the vocabulary; a name outside it stays a `RawTag`.** Every well-known
  name has a typed field and all three naming paths land on them. Left out on purpose: RIFF's `ISRC`
  (names the *source*), `EncodedBy` (who, not what, so `encoder` takes `Encoder` alone), and an album
  artist from `INFO` alone (`store::attribution` falling back to the track artist is the whole answer).
- **A name a revision gives twice is every one of them.** `Builder::listed` is the door for list fields
  (artist, album artist, genre, the credits): the first a revision gives replaces what an older
  revision left, each later one in the same revision is appended after `LISTED_APART_BY` unless already
  held. symphonia hands a multi-valued ID3 frame over as a `RawValue::StringList` with no standard tag,
  so `id3_list` maps `TPE1`, `TPE2`, `TCON`, `TCOM`, `TPE3`, `TEXT` and `TPE4` itself. A title, album,
  id or number keeps the last.
- **A text tag blank once trimmed is no tag, and what is stored is trimmed.** `tags::given` is the one
  door every text `StandardTag` takes into a `TagSet` slot, and leaves the slot as it was where nothing
  is left, so a blank frame written after a name does not unname it; ReplayGain values and numbers
  follow the rule (an unparsable, zero or oversized value leaves what an earlier frame gave, so an empty
  frame does not play a track at no gain). `tags::decibels` and `tags::peak` take a unit in any case and
  a decimal comma where it is the only separator. The newest revision wins a retagged date; ID3v2.3's
  `TDAT`/`TIME` are read by their own key (`Id3DatePart`), the clock never a date. A `COMM` frame whose
  description begins `iTun` is iTunes' private note (`is_an_itunes_note`).
- **An ID3 recording id arrives as a `UFID` frame nobody standardised into a tag.**
  `musicbrainz_identifier` reads it: key, owner against `MUSICBRAINZ_OWNER` (the whole check, since
  CDDB and others use `UFID` too), then the bytes as UTF-8. Tried only where `Naming::standard`
  answered nothing.
- **A Matroska file's length is the segment's, counted as well as read.** symphonia leaves `duration`
  unset for mkv. `matroska.rs` reads `Duration` and `TimestampScale` and also walks the clusters, so a
  streamed file with no `Duration` still has a length. The count is deliberately a *lower* bound:
  `Segment::duration` keeps the declaration wherever the clusters stay within it and prefers the count
  only where blocks exist past the declared end. A declaration too long is undetectable except for the
  first `A_OPUS`, `A_FLAC` or `A_VORBIS` track, counted exactly (`Segment::counted_frames_of`) where it
  is the track symphonia decodes. **A Vorbis packet's length is named by the packet before it**
  (`vorbis::Windows`, modes walked *backwards* from the setup header's framing bit as ffmpeg's
  `vorbis_parser` does). A seek lands on the exact frame only where the track timebase is the sample
  rate's reciprocal; Matroska's millisecond ticks make its landing approximate.
- **Opus is decoded by `opus-rs`, registered beside symphonia's decoders.** symphonia 0.6 demuxes Opus
  and decodes none of it, so `opus.rs` wraps `opus-rs` (a pure-Rust libopus port) as an `AudioDecoder`;
  `registry::codecs` is the `CodecRegistry` every decoder is built from. `Head` reads the identification
  header from the track's extra data (Ogg and Matroska `OpusHead` little-endian, MP4 `dOps` big-endian).
  Family 1 is the multistream decoder, its Vorbis-ordered channels written to the plane each position
  takes in symphonia's bit order; Matroska leaves a surround stream unplaced, so `opus::channels_of`
  places family 1 by its head (255 stays discrete). The header's output gain is applied as samples are
  written, and `R128_TRACK_GAIN` / `R128_ALBUM_GAIN` are read as ReplayGain 5 dB louder. **Where the
  pre-skip lands is the container's to say:** Ogg counts it *inside* its granules, Matroska shifts
  timestamps by `CodecDelay` (music at zero), MP4's edit list is what the box scan reads
  (`container::Carrying`). **Matroska's end is its last block's `DiscardPadding`**, which symphonia reads
  nowhere: the cluster walk keeps it and `Decoder::build` hands it to `Coded::trailing`, where `fill`
  reads a packet ahead and takes it off the last. **The declared length is counted:** every Opus packet
  says its length in its TOC byte, so `Segment::opus_music` is `playable`'s end and the length is exactly
  what decodes; the count is trusted only where nothing escaped it (bad lace, a packet over 120 ms, a
  segment not walked to its end), otherwise the window stays open-ended and the length is the
  millisecond count less the pre-skip (`Carrying::declared_before_the_music`). **A seek starts early**
  (`PRE_ROLL`) because CELT predicts each band's energy from the previous frame; `opus::pre_roll` is what
  `seek_reader` subtracts and the frames are discarded.
- **WavPack is read by `symphonia-codec-wavpack` and decoded through a wrapper of the codec crate's
  own.** `registry.rs` holds the one `Probe` and `CodecRegistry` every open goes through. The crate
  mishandles the extended bits of a 32-bit or floating block, so `wavpack.rs` rewrites each packet first
  and turns the answered integers into floats itself (a port of libwavpack's `float_values`; shifts
  held to a word, `SHIFTED_WITHIN_A_WORD`). **A hybrid WavPack is billed as the lossy codec it decodes
  as:** the prescan reads the first block's flags and `HYBRID` makes `coded_info` answer
  `wavpack::HYBRID_CODEC_ID`, read by `Codec::from_id` as `Codec::WavPackHybrid` (not `is_lossless`, so
  the badge, `is:lossy`, the verdict and the vault's `Kept` agree); the decode is unchanged. A `.wvc`
  correction file is not read: `symphonia-codec-wavpack` 0.1.1 gets a held zero's correction wrong.
- **A Vorbis setup header is walked before symphonia's decoder reads it.** An *ordered* codebook can
  count its lengths past 32 and index past symphonia's table, aborting the player in release.
  `vorbis::Vorbis` wraps `VorbisDecoder`, walks every codebook and refuses the decoder where a run lands
  past 32.
- **Monkey's Audio is read by a reader of the codec crate's own.** `ape.rs`'s `ApeReader` parses the
  header and seek table through `ape-decoder`, answers one packet per frame and seeks by frame. Its
  markers are `MAC ` and `MACF` (Monkey's Audio 11 floating; missing it, the probe opened the stored
  `RIFF` header as a WAVE). `Ape` decodes through `FrameDecoder` (per-frame checksum); a 32-bit stereo
  stream is refused, the crate narrowing the side channel before undoing it.
- **A WAVE too long for RIFF is read by a reader of the codec crate's own.** RF64/BW64 (`ds64`) and
  Sony Wave64 are not read by symphonia. `wide.rs`'s `WideReader` walks either to `data` (`Wide::chunk`
  is the one reading of a chunk header in both layouts), reads `fmt `, answers packets of at most
  `FRAMES_A_PACKET` frames and `MOST_PACKET_BYTES`, and seeks exactly; a data size past a source that
  states its length is cut to the bytes held. `riff.rs` walks the same layouts. The scan takes `.rf64`
  and `.w64` beside `.wav`.
- **Every PCM symphonia names is billed as PCM, and ADPCM as the lossy codec it is** (`Codec::Pcm`,
  `Codec::Adpcm`, the latter not `is_lossless`). **A 64-bit float source decodes as 32-bit float:**
  `container::representable` maps `F64` onto `SampleFormat::F32`, which holds more than any converter
  records; the declared depth stays 64 so `analyse` and the inspector say what the file is.
- **This build drops every priming itself and asks nobody else to.** `Decoder::build` makes its decoder
  with `AudioDecoderOptions::gapless(false)`, so the only trimming anywhere is `MediaInfo::playable`
  (only `symphonia-bundle-mp3` and `symphonia-codec-vorbis` implement symphonia's gapless).
- **A priming reaches `playable` from whoever named it, the reader or `boxes.rs`.**
  `container::priming` prefers `Track::delay`, `Track::padding` and `Track::num_frames` and falls back
  to the box scan where the reader named nothing. isomp4's reader names nothing, so the scan reads the
  `iTunSMPB` item, otherwise the first non-empty `elst` edit weighed against what `stts` says the decoder
  will emit; only for the `soun` track and where the media timescale is the sample rate. An iTunes MP3
  carries the same note in a `COMM` frame (`tags::itunes_gapless_note`, taken last by
  `container::itunes_priming`); `Priming::behind_a_decoder_delay` adds `MP3_DECODER_DELAY` to the delay
  and takes it off the padding.
- **A fragmented MP4's length is in its fragments, not its `moov`.** A DASH-style file (empty `mvhd`,
  `mdhd` and sample tables, audio in `moof`/`mdat`) declares nothing. The box scan reads `mvex/mehd`'s
  fragment duration, or the summed subsegment durations of the first top-level `sidx`, rescaled to the
  stream's rate; `coded_info` takes `Movie::fragmented_length` where the declared length is zero,
  before the Matroska segment's.
- **`playable` is decoded frames, and it may be open-ended.** `Decoder::build` drops
  `playable.start()` frames before the first block and takes `playable.frames()` as its limit, so
  `duration` is the playable count: what the seek bar, `mpris:length` and `heard.rs` read. `Priming`
  holds the count as an `Option`, since a reader can name a delay and not a length (an ogg over a pipe,
  a Xing header with a LAME delay and no frame count), and the priming must still go.
- **`Timeline` is told where the music starts, not how long the priming is.** It holds one
  `music_at: Timestamp` and maps a playable frame to a container timestamp by adding to it: zero for a
  reader that named the delay (symphonia's negative PTS), otherwise `Track::start_ts` plus the scanned
  priming (Ogg sets `start_ts` to `-delay`, so carrying them separately was wrong). A seek can land
  *ahead* of `music_at`, so `seek_reader` answers a `Landing` carrying `short_of_the_music` and
  `restart` drops that much. On a timeline not sample-accurate (Matroska) a landing on the first packet
  drops `MediaInfo::priming` whole, as the cold open does.
- **Every revision of a file's metadata is read, not only the newest.** Containers publish several
  (Matroska one per `Tags` element, isomp4 two, AIFF two) and symphonia's `skip_to_latest` discards the
  rest, so `tags::Revisions` drains the log once in `container::open`, oldest first, each revision
  absorbed on its own so `Naming` weighs one writer's key set. Visuals stay in the reader's log for
  `probe_cover_art`.
- **A picture is found once and copied once, and who copies is the caller's to say.** `probe::picture`
  is the one walk, handing what it found to a `taken` of the caller's choosing. **`probe_pictured` is
  the one open answering a file's tags and picture together**, the caller's `Picturing` saying how much:
  `Whether` copies nothing, `Copied` answers the bytes, and a row the sources stand in for answers
  `StoodIn`. The library scan reads through `probe_scanned` (`Whether`; `library.md` has why) and digests
  the first `PACKETS_DIGESTED` packets into a `PacketDigest` on the same open, what a moved file is
  known by once retagged. `TagSource::read` takes the `Picturing`, so no implementation can answer
  tags and picture through separate opens.
- **The engine's tag reader reads a picture on the open its tags came off wherever it can.**
  `catalog::whole` probes under `Copied`; `Shelf::keep_what_the_tags_saw` files the bytes where the
  picture is already waited on, `Look::Nothing` where there is none, else puts it in the `Spares` (at
  most `SPARE_ART_BYTES`, outside `ART_BYTES_HELD` until drawn). A row a sheet cuts from a file says
  nothing about the picture.
- **What the engine's catalog holds follows the file.** A failed read is `Look::Failed`, claimed again
  after `READ_AGAIN_AFTER`; `Nothing` is a read that found nothing and stands. Each entry keeps the
  file's size and mtime (`Stamp`), taken by the reader thread *before* it reads, and weighed at most every
  `LOOKED_AT_THE_FILE_EVERY`: a row due a look is queued by the lookup (`Held::to_look`, sent after the
  lock is let go, `Wanted::Look`) and the reader thread stats it and forgets a changed file, bumping the
  revision so a pane asks again. Nothing stats a file under the lock or on the caller's thread, so a
  stalled mount stalls the reader alone and the window's redraw and the bus go on with what was held.
- **A picture is weighed before it is copied.** `MAX_COVER_BYTES` is checked against `data.len()` before
  the `to_vec`, and an oversized visual is declined rather than failing the read: `choose` prefers a
  front cover then a picture typed `Other` or untyped. An icon, leaflet, back cover, disc or portrait is
  never drawn as the cover; a file holding only those answers `None`.
- **A hand-rolled parser never allocates what a header declares.** `riff.rs` and `matroska.rs` read
  `declared.min(MAX_…_BYTES)` (the `id3 ` chunk through a `take` growing only as bytes arrive), then
  seek absolutely past what the element claimed. A declared size is what the walk advances by, never
  what a `Vec` is sized to.
- **A source that cannot seek is spooled, and only one too long to spool is read from its head.**
  `container::open` reads a non-seekable source into memory up to `SPOOLED_AT_MOST` for at most
  `SPOOLED_WITHIN`; where the stream ends inside both it is opened as a seekable `Reading` with
  everything a file has. The wait keeps a slow remote stream from holding the first sample for its whole
  download; a stream still arriving goes on arriving onto disc. `spool::Spool` is an unnamed file under
  `/var/tmp` (unlinked at once; on disc because `/tmp` is tmpfs on Arch, Fedora and Flatpak; `TMPDIR` if
  set), holding the head, with a `resonate-spool` thread copying the rest up to
  `SPOOLED_ON_DISC_AT_MOST`. symphonia gets the `Spooling` side (waits for the rest, still
  `is_seekable() == false`, since a reader that thinks it can seek looks for the end at once) and the
  walks run over a `Cursor` of the head. **Once it has all arrived it can seek:**
  `Decoder::settle_the_spool` answers `None` until the copy reaches the end, then reopens the spooled file
  as a seekable `Unspooled`, seeks to the frame the old decoder had reached and takes its place; the
  engine asks every pass (`Engine::settle_what_was_spooled`) and republishes the digest so MPRIS's
  `CanSeek` follows. A spool that could not be made falls back to the `Replaying` head; one cut short
  never claims to be whole. A DSF, DSDIFF or Monkey's Audio stream is read by seeking, so one too long
  to hold is refused at once as `Error::ReadBySeeking`.
- **Until it can seek, a track keeps its stream.** A rebuild throws the ring and carry away and seeks
  the decoder back, which a track that cannot seek cannot do. So `Engine::seek` on such a track
  refuses with `codec::Error::NotSeekable` (`Cause::CannotSeek`), except a seek to its start (a
  restarting Previous among them), which opens the row again through `start`. A setting that rebuilds
  the stream goes through `rebind_where_it_stands`, which marks the rebuild owed;
  `settle_what_was_spooled` pays it the pass the spool settles, and the next track's `start` forgets
  it. A rebuild the graph forces (device gone, format renegotiated, stalled discard) still happens at
  once.
- **A prescan over a source that *can* seek reads through a window**, since the walks read ids and
  headers four bytes at a time: `Prescan::buffered` is `Prescan::read` over a `Window`, one
  `PRESCAN_WINDOW` array refilled by an absolute seek and a read, with a free `stream_position`.
  `rewind_the_source` puts the real stream back.
- **What can crash symphonia is guarded against where the prescan can see it.** Its WAVE reader
  multiplies a `fmt ` channel count into a `u16` and widens a short channel mask by a shift past 32 bits
  (wrapping in release, panicking in debug or fuzz). `container::open` refuses more channels than a
  WAVE layout holds (`Error::TooManyChannels`) or a mask `riff::a_mask_the_decoder_cannot_widen`
  (`Error::ChannelMaskNotRepresentable`) before symphonia sees either. symphonia's probe finds a
  container's magic byte by byte, so `prescan::opened_first` searches as far as the probe does
  (`PROBED_WITHIN`) and the guards weigh only `RIFF … WAVE` and `caff`; for any other container they
  stand aside. `caf.rs` reads the CAF reader's declared values first (`Error::PacketTooLarge`,
  `FrameCountNotRepresentable`, `PacketOffsetNotRepresentable`, `PacketTooLong` over
  `MOST_FRAMES_A_PACKET`, `PacketTableCut`).
- **A cue sheet's track is a window on a file, and the window lives in the decoder.**
  `Decoder::open_span` seeks to the span's first frame, stops at its last and rewrites
  `MediaInfo::duration` to the span's length, so everything upstream (seek clamp, seek bar, `heard.rs`,
  `mpris:length`) sees a short file; `self.position` stays absolute inside and only `position()` and
  `seek()` convert at the boundary. `probe_span` is `probe` through the same cut, so the tags a queue row
  draws and plays under come off one reading. A sheet's 1/75 s unit is a `sector`, never a frame
  (`Frames` means a PCM frame; `CueStamp::at` is the one conversion).
- **A window on a file is billed by the sheet that cut it, and the span names the row.** `confine`
  writes the row's own `TagSet` over the file's wherever a cut names the frame the span starts on
  (title, number and `REPLAYGAIN_TRACK_GAIN`). `CueFile::cut_at` matches on the start frame alone, and
  `CueTrack::titled` plus the scan's rows go through the same call, so catalog and transport cannot
  bill one row two ways. `cue::cut_for` is where the cut comes from: the `MediaInfo::cue` the file
  embeds, else the sheet beside a *local* file (`<stem>.cue`, then `<name>.cue`, via `sheets_beside`)
  whose `FILE` line names it as the scan's matching does (`library.md`) or whose only cut it is, else a
  sheet in one of the `DEEPEST_FOLDER_A_SHEET_NAMES` folders above whose `FILE` line reaches down to it
  (`cue::above`, the first `MOST_SHEETS_ABOVE`).
- **Where a track starts is a type.** `CueTrack::start` is a `CueStart`: `Written(CueStamp)` for a text
  sheet, `Sampled(Frames)` for a FLAC `CUESHEET` block, so a block is never rounded to a sector.
  `MediaInfo::cue` comes from a Vorbis `CUESHEET` comment, else the binary block `flac.rs` walks. A
  block carries no names, so `CueFile::billed_by` bills its tracks by the file's own album fields. The
  lead-out track is kept as `CueTrackKind::Data` so `audio_tracks` skips it while `span_of` reads its
  offset as the last audio track's end.
- **A file's chapters cut it as an embedded sheet does.** `container::chaptered` is `MediaInfo::cue`'s
  third source: symphonia's `FormatReader::chapters` (MP3 `CHAP`, Ogg `CHAPTERnnn`), else what the
  prescan read itself (`flac.rs` FLAC `CHAPTERnnn` comments, which symphonia throws away; `boxes.rs` an
  m4b's QuickTime chapter track or Nero's `chpl`). `chapters::cut` turns two or more distinct starts
  into a `CueFile` of `CueStart::Sampled` tracks; one chapter, or all at one moment, cut nothing.
  **A Matroska file's chapters are read by the prescan and hidden from symphonia**, whose reader
  refuses a file whose `EditionEntry` has no `EditionUID` (optional in the spec, omitted by ffmpeg):
  `matroska::read_chapters` takes them and `Voided` rewrites the element header as an EBML `Void` laid
  over the bytes symphonia reads by `container::Probed`.
- **A sheet's edge cases are settled once.** A track belongs to the file its `INDEX 01` is in, whichever
  `FILE` line its `TRACK` followed (EAC's gaps-appended sheets: `Reading::file` carries the track across
  with its start put back to the head of the new file). A pregap inside one file is the head of the
  track it leads into (`CueTrack::heard_from` is the one answer to where a row begins; the `PREGAP`
  command is not read). A track whose `INDEX 01` does not read, or that starts before the one ahead of
  it in its file, is dropped, so no two rows of one file overlap. A `FILE` line may name a folder below
  its sheet (`cue::folder_named`, at most two deep, matched in any case; the scan, the organiser and the
  decoder all ask it). Sheet-level `REM DISCNUMBER`, `TOTALDISCS`, `COMPOSER`, `SONGWRITER` and
  `CATALOG` go to every track naming no own, and a quoted value runs to the *last* quote on its line.
- **A cue sheet is read as far as it parses and never fails.** An unknown command is skipped and
  rubbish yields a sheet naming nothing. Only the source and the `LARGEST_CUE_SHEET` ceiling raise a
  `codec::Error`. Text is decoded by `resonate_core::text` (`rust-style.md`), and a renamed sheet is
  written back in the code page it was read in.

## DSD

- **A DSD file's carrier rate is what the rest of the workspace sees, whichever way it is delivered.**
  `SampleRate` is bounded to 768 kHz (`MAX_HZ`), so a DSD rate is never one; `DsdRate` carries the
  native rate and `carrier()` is `hz / 16`. A DSD64 file is 176.4 kHz `S24` stereo whether DoP-packed
  or decimated, so `Frames`, `Timeline`, the seek bar, the ring and `plan_output` need no special case.
  DSD512's carrier is out of range and refused at open with `RateNotRepresentable`.
- **`Packing` is the guard, not a rule to remember.** It rides on `MediaInfo` and `OutputPlan`, and the
  only constructor producing `DopMarked` is `untouched`, which writes `resample`, `equalisation`,
  `gain`, `dither_to` and `shaping` as literals, so no path leads from a packed source to a plan with a
  stage. `no_setting_the_pane_offers_can_produce_a_marked_plan_with_a_stage_in_it` walks every sink
  shape crossed with every setting. `OutputPlan::delivery` derives what to ask the decoder for from the
  same plan.
- **DoP is opt-in and off by default.** Nothing in ALSA or SPA advertises DoP support, and DoP sent to
  a DAC that does not decode it is full-scale white noise. So `EngineConfig::dop` defaults to false,
  and the gate is `SinkInfo::supports` exactly, never `best_spec_for`, whose fallback to the widest
  format would truncate DoP to S16 noise and whose layout fold would rewrite the markers.
  `Decoder::set_output_format` means *samples in this format* and clears the packing; DoP is reached
  only through `deliver`. `resonate explain` prints which was chosen and why not the other.
- **An all-zero DSD byte is negative full scale, not silence.** Three answers: the DSF final block's
  padding is truncated against the declared sample count; the decimator's FIR history is primed with
  `DSD_SILENCE` (`0x69`); the gap a seek opens is filled from `Packing::silence`, not PCM zeros.
- **The ring is told what silence looks like on the wire; it does not assume zeros.** `ring` takes a
  `Silence` (`Unmarked`, or `Marked` with a four-byte word and its alternate); `Silence::write` swaps the
  word with its alternate frame by frame, so the DoP marker keeps alternating through a gap.
  `Packing::silence` is the only constructor (`DopMarked` answers `0x05`/`0xFA` over a `0x69 0x69` pair,
  so a DAC holds lock through a seek's prime window; `Unmarked` writes nothing). `Silence` is `Copy` and
  allocates nothing, `RingConsumer::fill` being the RT thread. **A gap is a gap however it opened:** a
  ring running dry mid-track hands the graph a short chunk, which for DoP loses the DAC's lock, so
  `RingConsumer::fill` pads the rest of the quantum after an underrun, allowed by `Discard::finished`
  (set when a track's last audio is written, so a short chunk at a track's *end* is what it always
  was). `RtFault::Underrun` is raised either way.
- **What a starve cost is counted in frames, apart from the faults saying it happened.** The ring adds
  the frames the graph asked for and was not handed to one `AtomicU64` (an add, so the realtime rules
  hold), never for a track's ending short chunk; `RingMonitor::went_without` swaps it out, added
  *before* the fault is raised. `OutputStatus::went_without` is the total at the sink's rate and
  `Event::Underrun` carries it. It is exact where the fault queue is not (a fault with no room arrives
  as a bare count in `RtFault::Dropped`), which is why `RtFault::Underrun` carries no frames.
- **The marker a splice resumes on is the audio's, not the ring's, and both edges of a gap are made
  exact.** A DoP marker alternates by the decoder's *absolute* frame while `Silence` alternates by its
  own count, so silence between two audio frames could put two like markers together. The marker is read
  off the audio by its *sign* (`dop::packed` puts `0x05` in a 24-bit word's top byte and `0xFA`
  sign-extended, which `marks_negative` reads at any width; a hex dump looks right either way, the
  mistake shows once `retype` or `convert` touches the sample). `Silence::follows` is the leading edge
  and `RingConsumer::realigned` the trailing one: the first audio frame is peeked through an
  uncommitted `read_chunk` and one more silence frame written ahead of it where `Silence::would_write`
  says it would repeat.
- **A decimation is at the modulator's level unless the listener asks for PCM's.** DSD's full scale is
  50 % modulation, so a master decimates ~6 dB quieter than the same music in PCM.
  `EngineConfig::dsd_like_pcm` (the `dsd-like-pcm` key, off by default) makes `plan_for` add
  `DSD_MODULATION_DB` to the gain wherever it decimates, capped at a known peak and ridden by the
  true-peak guard where none is known. A DoP plan is never raised; `Command::SetDsdLikePcm` retunes in
  place.
- **A decimation is a conversion, and DoP comes back the moment nothing stands in its way.** A decimated
  stream is the carrier rate at `S24`, exactly what `MediaInfo.spec` says, so `plan_for` reads the
  source's `Packing`, not its spec: a `DopMarked` source delivered as samples is `OutputMode::Converted`
  and the graph is not asked for `NO_CONVERT`. `packs_again` is the other way: `retune` asks it (source is
  DSD, the open plan samples on the carrier's own spec, `dop_survives` against the bound sink) and
  rebinds into the marked plan where it holds.
- **A DST-compressed DSDIFF is unpacked into the stream an uncompressed one would be.** The `DST `
  chunk's `FRTE` gives the frame count, so the layout is known from the header. Only a decode walks the
  chunk: `dsd::unpacked` collects every `DSTF` and hands the DSD path `dst::Unpacked`, a `MediaStream`
  decoding whichever frame a read lands in, so the reader, DoP, the decimator and a seek see an
  uncompressed DSDIFF. A frame decodes alone, so a seek costs one frame. `dst.rs` is ISO/IEC 14496-3
  subpart 10 as FFmpeg's decoder reads it (at most `MOST_CHANNELS`; a frame whose segmentation is not
  the reference encoder's is refused as FFmpeg refuses it); the tests carry their own encoder.
- **A packet that will not decode is played as the silence it would have lasted.** A `DecodeError` on
  one packet hands `fill` the packet's length through its `PacketSpan` and the frames are marked
  `silent`, so the stream keeps its length and the position its clock. **Every such hole is counted, and
  a stream that is nothing but holes is refused:** `Decoder::holes` answers a `Holes`, and
  `SILENT_PACKETS_BEFORE_REFUSING` failures with no packet ever decoded answer
  `Error::PacketUndecodable`. `Decoder::refuse_holes` makes the first hole that error, for a reader
  that must have every sample: the vault calls it on what it keeps and reads back, so a damaged source
  is refused `NotValidated` and `--verify` fails an object with a hole.
- **A DSD file's tags are read whole.** The ID3 tag a DSF's metadata pointer or a DSDIFF's `ID3 ` chunk
  holds is read to the length its own header declares up to `MAX_METADATA_BYTES` (a flat cut made
  symphonia refuse a tag with a large cover, title and all); a DSDIFF's `DIIN` `DIAR`/`DITI` fill only
  what the ID3 tag left empty (`MAX_EDITED_TEXT_BYTES`). `dsd::described` answers tags and visuals
  together and `container::Pictures` is what every picture question asks. A failed DSD read is
  `Error::Io` reaching `next_block`, so the engine reports and skips the row rather than taking it as
  finished.
- **A DSD file's channels are placed where it says, never by their count.** DSF's `fmt ` chunk names a
  channel type and DSDIFF's `CHNL` an id per channel; `dsd::placed` turns either into a layout through
  `container::positioned_layout`. Ids out of the layout's order (`dsd::in_order`), an unlisted id, or a
  DSF type disagreeing with its count are `Discrete`: the planes are never reordered.
- **DSF bit-reverses and DFF does not.** DSF's `fmt ` chunk declares `1` for LSB-first and `8` for
  MSB-first, DFF is always MSB-first. The wrong way round yields distorted music rather than an obvious
  failure, so both orders decode the same tone in a test.
- **The decimator is the codec's own, the layering forbidding `resonate-dsp`.** A Kaiser-windowed sinc
  decimating by 16, 512 taps at DSD64 and **a kernel growing with the rate** (1 024 at DSD128, 2 048 at
  DSD256, `fir_bytes_for`), so its transition band stays as narrow in hertz and the audio band as flat
  (`the_kernel_grows_with_the_rate_so_the_audio_band_stays_flat_at_every_one`); it costs DSD256 stereo a
  fifth of a core, measured. Evaluated as a byte-indexed table of partial sums (no multiplies). A box average is the wrong shape (DSD's shaped noise rises steeply above 30 kHz), so the
  cutoff is 45 kHz absolute, rebuilt per rate, with ≥90 dB at the output Nyquist proved by a DFT of the
  designed taps. A DC blocker follows. The filter overshoots a step into all-ones, so full scale
  saturates at 8 388 607 rather than 8 388 608, which a 24-bit wire would read as negative full scale.
## Sources

- **`LocalFiles` is the local source's provider**, replaced under `SourceId::local()` by the vault's
  `VaultFiles` where a vault is open (`vault.md`); every other `MediaLocation` is refused by name unless
  a provider is registered (the seam is proved by a provider serving a track from memory in
  `crates/resonate-engine/tests/transport.rs`). A provider hands back a `Read + Seek + Send + Sync`
  stream and says whether it is seekable; a forward-only one is spooled (see Decode).
- **A non-filesystem provider is waited on for `Sources::OPENED_WITHIN` and no longer.**
  `Sources::open` asks `SourceId::local()` in line (a scan opens hundreds of thousands of files) and any
  other provider on a `resonate-open` thread, taking `Error::OpenTookTooLong` where nothing answered in
  time; the engine passes the row over, the tag reader reads it as nothing to say. What is given up on
  is not stopped, so a provider puts its own deadline on the network. **Its stream's reads are waited on
  the same way:** a `Deadlined` stream sends every read and seek to a `resonate-read` thread, waited on
  for `Sources::READ_WITHIN`; a read not answered in time is `io::ErrorKind::TimedOut` and leaves the
  stream stalled (every later call fails at once), so the engine fails the track rather than hanging.
- **Every track is opened off the engine thread.** `Engine::start` hands the row to an `Opening`: a
  `resonate-track-open` thread runs the whole `Unwrapped::open`, and the engine parks on its answer
  beside its commands, publishing `Loading`. Every command is answered while it waits (play or pause
  change whether the track plays once it lands, a seek moves where it will start, and a stop, load or
  skip drop the opening). **The command that began an opening is answered by its landing:**
  `Engine::dispatch` defers the reply of a command that left a new opening in flight,
  `answer_for_the_opening` runs the landing's outcome through `past_what_will_not_open` and the
  stranded-row handling, and a deferred reply is answered `Ok` once `OPENING_ANSWERED_WITHIN` passes
  or the opening is dropped (a later failure arrives as `Event::Failed`). `wait_for_the_graph` does not
  let go while a row is still opening. A failed landing nothing waited on is passed over while playing
  (`Engine::fail`) and an `Event::Failed` otherwise; a thread stopping without answering is
  `Error::OpenerStopped`.
- **A track that cannot seek is decoded off the engine thread.** A spool still arriving or a replayed
  pipe waits for bytes with no deadline. So `lending::Decoding` holds the decoder, and
  `Track::next_block` *lends* it, with its block, to a `resonate-decode` worker: the engine answers
  `Block::Awaited`, `fill` returns `Filled::WhenTheSourceAnswers`, and the engine takes the answer
  (`take_what_was_decoded`) at the next pass. While the decoder is away a delivery set meanwhile is owed
  and applied as it comes home, and `settle_the_spool` waits for it. Stop, load or skip drop the track
  and the worker with it. A seekable track, or one whose worker could not start, is decoded in line. A
  worker that stopped without answering is `Error::DecoderStopped`.
## Transport

- **Bit-perfect output takes precedence at every track boundary. There is no gapless playback.** On a
  track change the engine drains the ring and reopens the stream. Once the ring runs dry it asks the
  graph to drain and waits for `StreamEvent::Drained`, so the boundary falls where the graph says it
  played the tail out; `SinkStream::latency` on the wall clock is the fallback for a backend that never
  answers. A stream takes every instruction through one `StreamCommand`, keeping that handshake,
  `set_active` and `close` on one ordered channel to the loop thread.
- **Bluetooth headphones can be kept from cutting the start, off unless asked.** Headphones power their
  radio down when nothing is sent. `BluetoothWake` (the `bluetooth-wake`, `bluetooth-lead-ms` and
  `bluetooth-awake-s` keys) touches only a sink `SinkInfo::is_bluetooth` names. **A pause fades the ring
  out rather than standing the stream down:** the consumer feeds the graph real silence (zeros for PCM,
  the marked word for DoP) and the link stays up (`Output::awake_since`) until `awake_for` has passed
  (`let_the_link_rest`). **A stream starting on a link that has slept opens on silence:**
  `RingProducer::lead_in` pads frames before the ring is read. Whether the link slept is
  `Engine::sounded` against `LINK_NAPS_AFTER`, so a track change pays no lead.
- **Nothing the listener does cuts the waveform where it stands; it fades over `FADED_OVER`.**
  Deactivating a stream, closing it or dropping what the ring holds stops the music mid-cycle, a click.
  The fade is the consumer's, the ring already holding up to `buffer-ms` of rendered audio: `Fader` is
  shared by the ring's two ends, the producer asking for silence or sound (`fade_out`, `fade_in`) and the
  consumer walking its `level` toward it a frame at a time, then feeding silence without consuming
  (`is_quiet`). Away from a fade nothing is touched. A DoP ring is never scaled (a multiplied marker is
  noise) and goes quiet at once on its marked silence. **Pause** stands the stream down once the
  consumer is quiet (`finish_fading`), or after `quiet_within` where the graph never pulled. **Stop, a
  track change and every rebind** retire the output (`Engine::retiring`): it fades out and is closed
  once quiet, and `promote` opens no new stream until then, the client holding one playback stream at a
  time. **A seek in place** fades the old audio out (`fade_into_the_discard`) and the new in. **A stream
  opened mid-track** opens `Entering::FadedIn`; one opened at a track's start opens whole, so a
  bit-perfect first frame is untouched. Only a stream the graph is pulling fades
  (`Output::is_sounding`, `PULLED_WITHIN`).
- **A seek does not reopen the stream.** rtrb gives the producer no way to drop what the consumer has
  not read, so the ring carries a discard epoch: the engine bumps it and stops writing, the graph thread
  drains every slot on its next callback and acknowledges, and only then does the engine refill; one not
  acknowledging after `DISCARD_TIMEOUT` is taken as stalled and has the stream rebuilt. The graph is
  fed the stream's own silence (off the plan, not a `memset`) until the ring is half full again
  (`Output::primed`), so a seek costs one clean gap and is not reported as a fault. A seek while paused
  reopens instead: a graph not calling `process` can never acknowledge the discard, and reopening
  leaves the ring primed at the new position. A pause landing *after* an in-place seek is the same case
  caught late, read as `Unanswered::NothingPulling` rather than a stall. A sink switch and a
  renegotiation still reopen.
- **Leaving or returning to unity gain, and switching the equaliser, reshape the chain in place.**
  `retune` builds the wanted plan against the open stream. Where `OutputPlan::same_shape_as` holds it
  retunes the chain; where not but `OutputPlan::becomes_on_the_same_stream` does (same `stream`,
  `packing`, `remix`, `resample`, convolution by `Arc<Impulse>` pointer, and under a resampler or
  convolver the same `restoration`) it builds the new chain with `build_chain` on the engine thread and
  swaps it in under the ring, stream and consumer, so nothing reaches the realtime thread. The new gain
  stage is handed what the old applied (`Chain::ramp_gain_from`), so the level never steps, and decoded
  but unconverted samples are retyped with `AudioBuffer::retype`. Going back to a plan with no gain stage
  first ramps the running chain's gain to unity, holds the wanted plan in `Output::settles_into` and
  swaps once `Chain::is_ramping` says so (`finish_reshaping`); the equaliser eases likewise (`eq.md`). A
  newer command drops what was waiting. **A resampler and a convolver are carried across, not
  rebuilt:** a fresh resampler starts from silence and steps a steady level, and flushing a convolver put
  its whole decay (up to `LONGEST_IMPULSE`) into the ring ahead of the music. Where
  `OutputPlan::carries_the_front_into` holds, `Chain::take_the_front` lifts the stages up to the last one
  `Processor::is_carried_across_a_reshape` names, the rest is flushed into the ring and
  `build_chain_after` builds the new stages behind them; the swap waits for the room the stages *behind*
  the front hold (`Chain::max_flush_frames_behind_the_front`) while the pump stops filling
  (`Output::waits_for_room_to_reshape`). A restoration switched under a resampler or convolver still
  rebinds; a volume or ReplayGain change never does, a converting plan always carrying a gain stage.
- **A setting asking for the stream already open does not reopen it.** `Engine::keeps_its_stream` asks
  `plan_output` what the bound sink would open at now and weighs it against the open plan's stream and
  packing and `ring_capacity`; `reopen_where_the_stream_moves` rebinds only where one moved. The graph
  rate is not weighed so: `clock.force-rate` pins the graph, so switching it still reopens.
- **A renegotiation converges because the next plan asks for exactly what the graph answered.**
  `StreamEvent::FormatChanged` carries the spec the graph settled on and `downgrade` hands it to
  `plan_for` as the target, written into `OutputPlan::stream` whole; writing the source's channels back
  made the two trade turns forever. `MAX_RENEGOTIATIONS` is the belt: a fourth different spec fails the
  track with `Error::Renegotiation`, reset by `Engine::start`.
- **The run loop waits on every channel that can change its mind, and the ring sets the timeout.** A
  command, a sink announcement and the stream's events each wake it. The timeout is half what the ring
  holds, floored at `SHORTEST_TICK` and capped at `PUBLISH_TICK` (what `Published` is refreshed at for a
  60 Hz front end). Nothing streaming makes it `IDLE_TICK`, and a transport at rest (not playing, no
  sleep timer, graph not lost, sink list not stale, nothing filling, discarding, reshaping or ramping)
  parks `AT_REST_TICK`, every change that could move it arriving on a channel the `Select` wakes for. A
  disconnected receiver reports ready forever, so `changes` is dropped and `Output::deaf` set the first
  time one does.
- **`run` is two loops because a `Select` borrows its receivers.** The stream's receiver lives inside
  the `Output` every `&mut self` method replaces, so a `Select` from `&self` cannot outlive a body that
  rebinds. `Heard` is that set lifted out (`Engine::heard` clones the three `Receiver`s) and
  `Heard::is_what`, compared by `same_channel`, breaks the inner loop exactly when it changed.
  `unsafe_code = "forbid"` ruled out a self-referential field.
- **Enumerating sinks mid-stream happens on a thread of its own.** A `SinkChange` sets a flag, and the
  next tick hands it to `Surveying`: a `resonate-sinks` thread holding the backend's `Surveyor`,
  answering on a channel `Heard` parks on. The engine never waits on the graph. One survey is in flight
  at a time; a change announced meanwhile leaves the flag set and the landing answer asks again. Where
  the thread could not start, the engine enumerates in line only while no stream is open, under
  `SINK_TIMEOUT`.
- **A bind reads the published sink list; only an announcement refreshes it.** `select_sink` is on the
  path of every track change, non-in-place seek, settings change and renegotiation, so enumerating there
  would put a `SINK_TIMEOUT` before each against a daemon that stopped answering. `note_sink_changes`
  is read before the bind as well as on the tick; the standing reasons to ask the graph again are a
  stale flag, an empty list and a change stream gone, and where the ask fails with a list still
  published the bind takes it. **No bind asks in line where the survey answers for the list**
  (`the_survey_answers_for_the_list`: a survey thread and a change stream). A list nothing announced a
  change to since the survey last answered is current, an empty one included, so a bind takes it and
  an empty one is `NoSink` at once; a stale list still holding devices is bound from as published and
  the survey's answer moves the stream through `follow_the_sink_it_would_choose`. A list known wrong —
  stale or with a survey out, and empty, or with a row waiting for a device or the graph
  (`the_survey_will_answer_for_a_list_known_wrong`) — is not bound from at all: `wait_for_the_survey`
  keeps the row at its frame in `unbound`, publishes `Loading`, marks it waiting for a device unless the
  graph is lost, and the command answers `Ok`; the answer binds it through
  `bind_the_row_waiting_for_a_device` or `bind_the_row_the_graph_let_go`. `wait_for_the_graph` marks the
  list stale only while no survey is out, so the answer it waits on is not made stale by the wait
  itself. A first survey at startup that failed leaves the list stale
  (`a_load_onto_a_list_with_no_devices_is_refused_without_asking_the_graph_again`,
  `a_load_while_the_survey_is_out_is_answered_at_once_and_bound_once_it_comes_back`).
- **Every stream is opened off the engine thread.** `Backend::opener` hands out an `Opener` (for
  PipeWire the cloneable `Survey`, whose `open` waits up to `STREAM_OPENED_WITHIN` for the loop), and
  `Streaming` is a `resonate-stream-open` worker taking one `Asked` at a time. `promote` hands it the
  request and the ring's consumer and the transport stays `Loading`, every command answered meanwhile;
  `land_the_stream` takes the answer on a later pass, `Heard` waking on it. Each `Output` is numbered
  (`Output::made`, from `outputs_made`), so a stream landing for an output since retired or rebound is
  closed (`close_unwanted`) rather than taken, and a failure for one is only logged. **One open is in
  flight at a time, and none is asked while one is:** a stray's `Close` must reach the loop before the
  next `Open`, `Close` acting on whichever stream the client holds. `wait_for_the_graph` does not let go
  while an open is in flight, so an open refused `Disconnected` is read as the graph's. Where no worker
  could start the open runs in line as before. A test reading `opens` waits for it rather than for the
  state the bind published, which comes first.
- **The engine decides which device the stream is on, and follows the default itself.** A playback
  stream carries `node.dont-move`, so WirePlumber never relinks it (its `follow-default-target` moved the
  stream while `OutputStatus::sink` named the old device). Every sink-list refresh ends in
  `follow_the_sink_it_would_choose`: where `chosen_sink` (the reading `select_sink` binds through: the
  named device, else the default, else the first) answers a device other than the one the output
  opened on, the row is bound again at its position. A desktop's per-application device picker cannot
  move the stream; the settings pane chooses.
- **A command never leaves the transport playing nothing.** A rebind or start retires the old output
  before it binds, so a failure for the row's own reason (`Convert`, `Decode`) would leave no stream
  while the transport plays. `carried_on_past_a_stranded_row` sends an error naming a track
  (`Error::track`) that leaves the engine `is_stranded` through `fail` (reported, skipping on) and the
  command answers `Ok`; a device's error (`NoSink` to a `Load`) is answered as before.
- **A graph letting go of the ring is waited for once, and fails the track the second time.**
  `RingProducer::is_abandoned` says the consumer was dropped (a daemon restart). `watch_graph` closes the
  output, keeps the row with the heard position in `unbound` and publishes `Loading`; the sink survey
  runs and the landing answer binds the row again at that frame (`bind_the_row_the_graph_let_go`;
  `wait_for_the_graph_in_line` only where no survey thread could start). It waits `GRAPH_BACK_WITHIN`
  before failing the row; a pause or stop ends the wait, another row does not: a track change or *Play*
  landing while the graph is away fails its bind with `Disconnected`, `LoopStopped` or a `Daemon` error,
  which `parked_for_a_device` reads, while `graph_lost` stands, as the graph's and not the row's. A bind
  refused with `Disconnected` before `watch_graph` has seen the ring abandoned starts the wait itself
  (`graph_lost_again` is the one guard both share). A graph letting go again within that window raises
  `Error::LoopStopped` rather than looping for ever (`graph_last_lost`). Asking the sinks first matters: a bind succeeds against the stale list and fails
  only when the stream opens. A disconnected *event* channel stays `Output::deaf`, since a graph can
  stop reporting and go on pulling. A failing `backend.open` takes the whole `Output` with it (the
  consumer went into the call and cannot come back).
- **A device going is waited out, not billed to the row.** `Engine::fail` first hands its error to
  `parked_for_a_device`: `NoSink`, `SinkGone` and `StreamFailed` with a track open close the output, keep
  the row in `unbound` at the heard position, publish `Loading` (`Paused` where paused), mark the sink
  list stale and raise `Event::Waiting` rather than `Event::Failed`. Every sink-list answer then ends in
  `bind_the_row_waiting_for_a_device`, binding again where the row was heard: on the fallback where the
  desktop has one, on the next device to appear where it has none. A stream failing again within
  `GRAPH_BACK_WITHIN` of the last loss is the row's after all (`device_last_lost`). A `Load` answering
  `NoSink` to its caller is unchanged. With no output, `Engine::position` answers `unbound` where it is
  set, not the decoder's place (a buffer ahead of what was heard).
- **A fill is a slice, not a loop to a full ring.** `Engine::fill` decodes and writes for at most
  `FILLED_IN_ONE_GO` and answers `Filled::ForNow` where it stopped with room left; `pump` keeps that
  as `fill_owed` and `budget` answers no wait while it stands, so a Pause or Stop is not held behind a
  deep ring's priming.
- **The PCM ring carries `u8`, not `f32`.** An `f32` ring would convert every stream and break
  bit-accuracy for 32-bit sources. `ring_capacity` clamps the buffer setting between `MIN_RING_FRAMES`
  (or `BLOCKS_A_RING_HOLDS` of the widest block one pass writes, `Chain::max_process_frames`, where more)
  and `LARGEST_RING` bytes, so a mistyped `buffer-ms` sizes a ring rather than aborting; the block is
  weighed because the converting `fill` writes only while the ring has room for one and `primed` waits
  for half the ring (a heavy upsample could otherwise sit in `Buffering` with no error). A flush is not a
  block (a convolver's tail is handed on from `staged` a ring's room at a time, `drain`). **The ring's
  depth is the engine's, never the graph's.** Every stream asks `LatencyRequest::Auto`: a quantum is a
  few milliseconds shared by every client on the device, and a ring of 100 ms to a second written into
  it would drag the whole graph to PipeWire's largest quantum. The depth decides how long a volume,
  ReplayGain or equaliser change waits to be heard and how much an opening stream primes.
- **A track change does not start the transport; a command does.** `Engine::start` opens the row the
  queue is on at whatever the transport was doing, so `Next`, `Previous` and removing the playing row
  leave a pause in place (what MPRIS says). `Load { autoplay }`, `Play`, `Command::JumpTo` and an
  `Insert` asked to be heard set `playing`. A track failing mid-play still advances into playback.
  **Nothing to hear sets nothing:** a `Load` with no rows leaves `playing` clear, and `Play` over a queue
  with no current row answers `QueueEmpty`. **A toggle weighs what is heard, not what was wanted.** A
  load or `Next` answered `NoSink` leaves `playing` set over a track with no stream and nothing waiting
  for a device (`Engine::is_stranded`), and `TogglePlayPause` plays there, binding the row; a row
  waiting for a device is not stranded, so the toggle pauses it.
- **A track change nothing will be heard through opens the file and binds nothing else.**
  `Engine::start` reaching a paused transport records the frame it would bind at in `Engine::unbound`
  and stops, so a run of skips costs one `Unwrapped::open` a row rather than a sink selection, ring, DSP
  chain, half-ring decode and `Backend::open` each. Every `rebind` while `unbound` is set records the
  frame instead; `Engine::play` is the one place spending it, and a bind failing anywhere leaves
  `unbound` set so the next `Play` tries again. The row still publishes what it is but not
  `PlayerState::output`, which stays `None` until something is bound.
## The queue

- **A queue position is an index into the play order, not the load order.** `PlayerState::queue_position`,
  `Command::JumpTo`, `Command::Insert` and `Command::Remove`'s `Span` all mean the rows a queue pane
  draws, so they stay right under shuffle. **An edit that names rows by position can say which queue it
  meant**: `Player::send_if_the_queue_is_still` and `request_if_the_queue_is_still` carry the
  `Queued::revision` the gesture was made against (`Request::queue_seen`), and `dispatch` answers
  `Error::QueueChanged` (`Cause::QueueMoved`, the toast *The queue changed before that could happen*)
  instead of applying it where the revision has since moved, so another client's insert or removal
  between the draw and the click moves or removes nothing
  (`an_edit_made_against_a_queue_another_client_changed_is_refused_and_touches_nothing`). The window's
  `PlayerModel::send_by_position` is the revision it last polled, and sends the queue's `Remove`, `Move`
  and `JumpTo`; a put back (`Insert`) and the bus and MCP, whose clients name a row by its id, are not
  guarded. `Queue::position` is the load-order index, published as
  `PlayerState::loaded_position` for the reader drawing the order the queue was *loaded* in. A file no
  scan has seen takes its `TrackId` from `unclaimed_id`, counting down from `u64::MAX` past the ids the
  queue holds (`Unclaimed::beside` is that walk taken once for a whole run). **A file the catalog holds
  is queued under the catalog's id wherever there is a catalog to ask:** `Host::held_as` answers the
  library row for a location and span, `AddTrack` and `OpenUri` ask it before they mint, and the
  binary's `mpris::claimed_by` does the same for files `resonate play` and the window are handed. A
  resumed row likewise (`Resumable::held`).
- **A queue row's id names that row and no other, and the queue makes it so.** `one_id_each` runs over
  everything `Queue::load` and `Queue::insert` are handed: a row whose id is already claimed gets a
  minted one (`Unclaimed::claim`). It must be the queue, not the callers, which mint against the last
  *published* queue; a duplicate id made `Tracks` publish one object path twice so `RemoveTrack` and
  `GoTo` could not reach the second row.
- **A queue is stamped by the rows it holds, not the ids they arrived under.** `stamp_of` hashes each
  row's location and span; `Queue::rows_changed`, `RootView::play_playlist` and the binary's
  `play_queue` read through it, so a row the queue renamed on the way in still stamps as its playlist.
- **What was queued is kept apart from what is playing.** `Queue` holds `order` (the loaded album or
  playlist, the only list shuffle, a wrap under `RepeatMode::Queue` and unshuffling touch) and `next`
  (rows somebody asked to hear, in asking order). `after` says how many rows of `order` are drawn ahead
  of `next` and `Seat` whether the row heard is `order[after - 1]`, `next[0]` or nothing, so the list
  every reader draws is `order[..after] ++ next ++ order[after..]`. Playing moves through `next` before
  `order` goes on, and a heard or skipped-past row of `next` *leaves the queue* rather than joining the
  playlist: `PlayerState::queue_stamp` is `Queue::playing_from` (the stamp of `order`'s rows alone, so
  `Library::playing_playlist` still badges the playlist while a queued row plays) and `Queued::stamp`
  is every row, what `Keeping` weighs. *Previous* or a jump into the playlist keeps what was queued
  waiting. A load replaces `order` and keeps `next`; a resumption carries it as `Resumption::next`.
- **Loading replaces what is playing; queueing adds to what waits, and where a row lands is a
  `Placement`.** `Command::Insert` carries `Placement::Next` (front of `next`, behind a queued row being
  heard), `Queued` (end of `next`) or `At(row)` (a row of the drawn list, joining `next` wherever the row
  before it is the one heard or a queued one and `order` otherwise). With nothing heard every placement
  lands in `order`. A row put into `order` takes the queue out of `Library::playing_playlist` by
  restamping, so the badge returns if the row is dropped again.
- **`Command::Insert` carries whether to hear what it queued.** `play: true` spends the play-order row
  `Queue::insert` returns through the `Engine::hear` that `Command::JumpTo` takes, so the two cannot
  disagree about starting a row (MPRIS's `SetAsCurrent` is one command, no row computed off a stale
  published queue). `RootView::queue` asks for it wherever `PlayerState::current` is `None`.
- **Coming back is a command of its own.** `Command::Resume` carries a whole `Resumption` (rows as
  playing, where each was loaded, the row the queue was on, the frame into it, shuffled or not), the
  only caller of `Engine::start(at)` passing anything but `Frames::ZERO`. A frame past the end of what
  the row now holds opens the row at its start. A resumption opens its row and stops (`start` on a
  non-playing transport records the frame in `Engine::unbound`), so it costs one `Unwrapped::open` and
  one decoder seek and spends the bind on the first `Play`. It is not a seek (`Seeks` is not stepped).
  Ids are not kept; `Queue::restore` mints through `Unclaimed::beside`. Only the bare `resonate`
  resumes, and turning `resume` off discards what was kept rather than merely ceasing to add.
- **The unshuffled play order is kept apart from the load order.** `Queue::unshuffled` is what the
  queue plays with shuffle off: a drag, sort or play-next while unshuffled edits it along with `order`,
  and toggling shuffle comes back to it. Under shuffle a removal drops the row and a queued row lands
  after the row it follows. Not kept across runs.
- **Removing the playing row moves on as a skip would.** `Queue::hand_on` is `advance` without the
  repeat-track hold: the queued row next, else the row after, else under `RepeatMode::Queue`
  `wrap_to_the_start`. Only a queue left empty stops.
- **A queue comes back in the order it was loaded and plays in the order it was playing.**
  `Resumption::rows` is the load order and `Resumption::order` the play order over it.
  `resonate-core::plays_in` makes trusting a stored order safe: one naming each row exactly once is
  taken as it stands, anything else gives back the load order. `Queue::restore` writes the shuffle flag
  rather than calling `set_shuffle`, which would reshuffle. Volume and repeat mode are deliberately not
  kept: they are settings a run makes, not a place it reached.
- **What the transport was doing is sampled as a play is.** `Keeping` reads the same `PlayerState` and
  published queue as `Listening`, answering `Keep::Queue` (`Queued::stamp` moved: the whole run),
  `Keep::Order` (`Queued::revision` or the shuffle moved with the same rows) or `Keep::Place`, only
  where the row changed, the position moved `KEPT_EVERY`, or the transport is at rest elsewhere than the
  place kept; a sample with no queue position (a queue played to its end) answers none, so the last real
  place stands. Only the first allocates a `Resumption`. `Resumable`, `Resumption` and `Reordered` are
  `resonate-core`'s because the engine builds them and the catalog stores them.
- **A play is what was heard, not what was started, and the transport is sampled, not hooked.**
  `Listening` is handed a `PlayerState` beside the queue and accumulates the frames the position advanced
  while playing (nothing for a sample whose `PlayerState::seeks` differs, or a step over `A_SEEK` the
  engine did not count as a seek), answering once per visit when half the track or `COUNTS_AS_HEARD`,
  whichever is shorter, has gone by. A seek keeps the visit open (a seek back after the mark does not
  count twice) except a seek back from within `A_SEEK` of a known end, a repeat wrapping, which begins
  another; a position going backwards with no seek counted starts the count again. A sampler because
  the write behind it is SQLite: `Library::track_played` would leave the run loop waiting behind a scan
  holding the writer, so `RootView::count_a_play` and `resonate play`'s loop (`HEARD_SAMPLE`) run it off
  the engine thread; a run with no catalog counts nothing.
- **A visit says how long it was heard, twice.** `Listening::heard` answers `Counts(Played)` at the
  threshold and `Settles(Played)` once when a counted visit ends. A visit that never counted answers
  `Passes` as it ends where it reached `PASSES_AT_LEAST`, so a flick through the queue is not listening
  and twenty seconds of each of forty songs is; `Library::passed` keeps it in `passes` apart from the
  plays (listening time counts it; a skip is still not a play). The pair is joined by an id, not a
  location: `Library::track_played` answers the `Listen` it wrote and `Library::listened` spends it, so
  a failed write keeps nothing and a settle cannot land on the wrong row. A counted visit also answers
  `Hears` every `TOLD_EVERY`, so a window closed mid-track loses at most that much; `Listening::leaves`
  settles whatever visit is open.
- **The sleep timer is the engine's, because a headless run wants one too.**
  `Command::SleepUntil(Option<Until>)` carries `After`, `EndOfTrack` or `EndOfQueue`, `None` cancels, and
  the deadline lives on `Engine`. It **pauses** (somebody who falls asleep should wake where they were).
  The end-of variants need only the edges in `skip`, and the wrap is the one that would be missed, so
  `Queue::wraps_next` reads the two fields `advance` decides the wrap from. **It fades the music out over
  its last `SLEEP_FADES_OVER`:** `sleep_is_due_in` answers what is left, and `fade_toward_sleep` asks the
  ring for silence over exactly that. A timer cancelled or pushed back mid-fade brings the level back; a
  seek or restart in place lifts the fade (`lift_the_sleep_fade`, `sleep_lifted`). The timer outlives a
  track change, seek, pause and new load: it is a timer on the listener, not the transport. A delay is
  held to `LONGEST_SLEEP` so a huge value cannot overflow an `Instant`.
- **A skip while one track repeats repeats the queue instead, unless the listener said otherwise.**
  `Command::Next` is a person's skip, and so is `Command::Previous` going back a track (a track's end
  reaches `skip` as `natural`), so `skipped_by_hand` turns `RepeatMode::Track` into `RepeatMode::Queue`
  before moving. `EngineConfig::skip_under_repeat` (`SkipUnderRepeat::KeepsRepeatingTheTrack`, the
  `skip-repeats-queue` key) is in the engine so a media key, MPRIS `Next` and `resonate play`'s `n` all
  obey it. A jump to a chosen row is not a skip.
- **Previous restarts the song once past its opening, unless the listener said otherwise.**
  `PreviousRestarts::starts_the_track_over` reads the heard position (the decoder less what ring, chain
  and sink still hold) against `PreviousRestarts::OPENING`. Past it, under `RestartsTheTrack`,
  `Command::Previous` seeks to the start and leaves the queue where it is; within it, on a track no
  longer than it, with no track open, or under `AlwaysGoesBack`, it retreats as always. A track of
  unknown length is weighed on the heard position alone. A restart is a seek (steps `Seeks`); a retreat
  runs `start`. Default restarts; `previous-restarts` is the key, live through
  `Command::SetPreviousRestarts`.
- **A relative seek past the end moves on, as the bus's `Seek` does.** `Command::SeekBy` landing at or
  past a known length is `Command::Next`, so a held arrow key reaches the next row rather than a
  `SeekOutOfRange` toast. An absolute `Command::Seek` past the end is still refused. A seekable stream
  declaring no length seeks like any other (`Decoder::seek` lets the reader try).
- **The published position never steps back within a stretch of listening.** The sink's latency is known
  only once a stream reports it, so a rebind or seek would publish a position a latency behind, which
  `Listening` read as a new visit. `heard_position` holds the published value at or above the last,
  released only by a seek that landed, a track opened and a stop.
- **Giving up is counted per run of failures, not per track opened.** `Engine::fail` stops the transport
  once more rows have failed than the queue holds; the count is put back by a track played to its end, a
  stop, or a person choosing a row, not by a stream opening (a daemon taking every stream then dropping
  it would otherwise loop a repeating queue for ever).
- **A row that will not open is passed over, not stood on.** A deleted file, an unmounted disc or a
  playlist row `--tidy` would drop answers `Error::Decode` out of `Unwrapped::open`, and
  `past_what_will_not_open` is what every command meaning *play something* runs the start through, so
  where the transport was meant to play the failure goes to `Engine::fail`, which reports and skips.
  `skip` asks whether the *queue* is on a row rather than whether a track is open, and `settle` hands a
  failed natural skip to `fail` rather than `stop`. `Error::track` is how `Event::Failed` names the row.
  A paused transport is left on the row.
## What the engine publishes

- **An event is owed, not dropped, when nobody has drained the channel.** The `EVENT_SLOTS` fill while a
  front end is busy; what does not fit waits in `events_owed`, in order, for `hand_over_what_is_owed`
  at the end of every pass, so a run of track changes cannot push out the `QueueFinished` headless
  `play` waits on. Past `EVENTS_OWED_AT_MOST` the oldest goes with a warning. An `Underrun` is the one
  event let go, told at most every `UNDERRUNS_TOLD_EVERY` with the missing frames summed into it.
- **What the engine publishes is one `Published` bundle, not a growing argument list.** It holds the
  `PlayerState`, `OutputSettings`, `StreamDigest`, the queue, the sink list and the `Tapped` the
  visualiser reads, each behind its own lock so a 60 Hz poll clones a pointer. The queue is a `Queued`
  (rows in play order, where each was loaded, the revision) under *one* lock, so a reader cannot pair
  rows with an order from another moment; the queue and digest are written *before* the `PlayerState`
  advertising them. `PlayerState::seeks` is a token `Engine::seek` steps where the seek landed and
  nowhere else (a refused seek and a track change step nothing), so a reader is told the position moved
  on purpose; `resonate-mpris` emits `Seeked` from it. `PlayerState::sleeping` carries what is *left* on
  the timer, so a front end keeps no clock. The queue is republished only when `Queue::revision` moves.
- **A command is answered once the state answering for it has been published.** `Engine::dispatch` keeps
  each reply as an `Answer` and `Engine::answer` sends them after `publish` at the end of the same
  pass, so `Outcome::wait` means *the published state already says so*. `Reply` is which of three ways
  a command came: `Player::send` is waited on by nobody and a refusal is announced as
  `Event::CommandFailed`; `Player::request` hands the refusal back in its `Outcome` and announces
  nothing; `Player::settle` answers a `Landing` saying only that the command was applied, while a
  refusal is still *announced*. `resonate-mpris` settles every call moving the engine through
  `Shared::settle`, waiting `SETTLE`, because zbus emits `PropertiesChanged` by calling the getter the
  moment a setter returns `Ok`, so a setter returning first announced the value just changed away
  from. A wait that runs out is a debug record, not a refusal.
- **What a setting is *set* to comes from the engine, not the pane that set it.** `OutputSettings` is
  the engine's whole configured output, republished as a new `Arc` only when one moves, so the settings
  pane marks the chosen option from what is in force. It is apart from `PlayerState` because the sink
  *name* is the choice and `OutputStatus::sink` the `SinkId` actually opened. `EngineConfig::sink` being
  `None` means *follow the default*, so `Setting::Sink` carries an `Option<NodeName>` and
  `config::clear` removes the key.
- **The inspector never reads the playing file a second time.** The engine samples
  `Decoder::last_packet` through a `ProfileBuilder` and publishes a `StreamDigest`; `resonate-ui`
  renders it with no dependency on `resonate-codec`. The box layout is the one thing opening the source
  again and the inspector's to draw, so `inspected` answers `None` where `probe_boxes` fails rather than
  failing a track that would have decoded. `Player::analyse` decodes the playing row whole on the
  window's background executor and only while the analysis pane is in front (`analysis.md`).
- **What is heard is tapped where it is written, and read back by where the graph has got to.**
  `Tapping` sits on `Output` beside the ring and `Engine::fill`, `convert` and `drain` hand it exactly
  the frames `RingProducer` took (after equaliser, gain and dither), as f32 left and right at the sink's
  rate. A `Tap` is a power-of-two ring of `AtomicU32` whose slots are a `OnceLock` laid down by the first
  frame recorded while somebody listens, so a run never opening the visualiser allocates none. Nothing
  locks and the engine never waits on the window: a write claims its frames behind a release fence and
  publishes the count with release; a read acquires, reads, rereads the claim and silences any frame
  the writer may have gone round onto. Where the graph has got to is an anchor `Engine::publish` fixes
  every pass (frames tapped less what the ring holds less `SinkStream::latency`) written through a
  three-atomic seqlock; `Tap::around` runs on from it by the clock (never more than
  `RUNS_AHEAD_AT_MOST`) and hands back the frames *centred* on the one heard. `Player::listen_in` is the
  switch (unlistened, a block costs a load and a store; listening again marks `valid_from`, so what the
  ring held reads as silence). `Player::tap` answers `Tapped`: `Nothing`, `Samples`, or `Markers` for a
  DoP stream.
- **A profile holds at most `MAX_WINDOWS` points, whatever a packet claims to span.** Windows come from a
  packet's declared duration, so an absurd `dur` would ask for billions; once full, the open window is
  abandoned, bounding the `Vec` and the walk on the engine thread as in `probe_stream`.
- **A queued row nothing plays is read once, off the audio path.** `Player::media` answers from a bounded
  catalog `READERS` (four) `resonate-tags-<n>` threads fill with `probe`, so a source that does not answer holds back
  no more than the reader it holds (`a_source_that_does_not_answer_holds_back_no_more_than_the_reader_it_holds`); a miss requests the read and answers nothing rather than
  blocking on a disc, and `Player::media_revision` moves when one lands. It lets `resonate-ui` draw a
  title, artist, length and cover for an unscanned file without depending on `resonate-codec`, and is
  what `resonate-mpris` answers `GetTracksMetadata` from, under one deadline for the whole call.
  `Player::art` is the picture from the same entry; the two are bounded apart (`ROWS_HELD`,
  `ART_BYTES_HELD`, so a picture drops back to unasked while its tags stay), asked on channels of their
  own, and a picture is read only when no name waits. What is read is a `Row`, a location *and* the span
  the queue row names; both live in one `Entry` under the location, so the cover is asked once however
  many rows a sheet cuts and eviction takes a file with all its rows.
- **Seeing a row unasked and claiming it are one operation under one lock.** `Claim` is what the shelf
  answers, so the 60 Hz poll and the bus thread cannot both enqueue one location. The guard never leaves
  `Shelf` (a `match` scrutinee holds its temporary for the whole match, so a `Catalog` locking the shelf
  itself would deadlock on the arm that asks), which is why `Shelf::claim_tags` takes and drops the lock
  inside a call of its own.
- **What may be asked at once is bounded, and a row turned away is asked again.** The request channels
  hold `ROWS_ASKED` names and `PICTURES_ASKED` pictures; an ask that would not fit answers
  `Sent::Backlogged` and the claim is *released* back to `Unasked`, so the next poll asks for what is
  still on screen, not every row a scroll went past.
- **The catalog is ordered by use, not searched for the stalest.** `Held` keeps a `BTreeMap` from the
  use clock to the row (`order`) and a second holding only rows with a picture (`pictures`), both
  re-keyed whenever a row is looked at, so eviction and `trim_pictures` take a step from the front.
## DSP

- **The chain is remix, lossy restoration, resample, convolver, equaliser, gain, the true-peak guard
  and dither; the equaliser's own rules are `eq.md`'s.** The equaliser sits after the resampler because
  a biquad's shape is warped by its design rate (designing at the source rate would sound different
  per track), and before the gain because the volume slider is the listener's last word and should
  attenuate a boost. The convolver comes before it (both linear, so the order changes nothing heard)
  so it is part of the front a reshape carries. An equaliser in force makes the plan `Converted`; a
  DoP-packed stream refuses it outright.
- **The resampler's levels are one filter design at four lengths; High is the default.**
  `Quality::params` is the whole difference (`half_taps`, phases, cutoff, Kaiser β); the settings pane
  prints those numbers on hover, which is why `SincParams` is re-exported through `resonate-engine`.
  `VeryHigh` is built to beat SoX's `rate -v` on paper, and `very_high_beats_sox_very_high_on_paper` and
  `each_quality_meets_its_alias_rejection_floor` hold it, which only an f64 carrier can show.
- **The resampler's phase is a setting of its own, and a shaped phase is designed once a run.**
  `FilterPhase` is `Linear`, `Intermediate` or `Minimum` (SoX's `-L`, `-I`, `-M`), on `ResamplerConfig`,
  `EngineConfig` and `OutputSettings`, as `Command::SetFilterPhase`, the `filter-phase` key and
  `--filter-phase`. A shaped phase is designed in `phase.rs` from the same quality's linear kernel
  (sampled at `SAMPLED_PER_TAP`, transformed at `CEPSTRUM_PADDING` times its length, folded through the
  real cepstrum into the minimum-phase response, and for `Intermediate` averaged with the linear
  phase's delay). An intermediate response is longer than the linear kernel and is kept from 1.25
  half-widths before its peak to two after. A design is kept in `DESIGNED` for the run, keyed by the
  `SincParams` and phase, being the same prototype whatever the ratio.
- **The resampler's reach is two numbers, a shaped kernel not being symmetric.** `Reach` holds `behind`
  (source frames of history an instant weighs) and `ahead` (frames it waits for); the history is primed
  with `behind` zeros and flushed with `ahead` zeros, and the latency is `ahead` frames, so a track is
  exactly as long whatever the phase (`a_track_keeps_its_length_whatever_the_phase`).
- **The kernel is stored polyphase, the history planar, the read position exactly rational.** The
  position is a `whole` sample index and a `phase` numerator over the phase count, stepped by integer
  arithmetic; the count is `SampleRate::ratio_to(output).numer` (one for an integer ratio, 160 for 44.1
  onto 48 kHz), since an f64 accumulator stepped by `1 / ratio` smeared those 160 into over a thousand.
  `Tabulated` builds every phase's weights once in `Resampler::new`, each phase divided by its own sum so
  every phase passes DC at unity. The last table is kept behind an `Arc` keyed by the rates,
  `SincParams` and phase (only up to `KEPT_TABLE_BYTES_AT_MOST`), and `TABULATED_WEIGHT_BYTES_AT_MOST`
  is sized so every pair of the common rates within 32:1 tabulates at every level; a rate no device
  offers reaches `Kernel`, which reads each weight off a four-point Lagrange stencil. The history is one
  f64 plane per channel, a frame's channels share one pass over the weights in pairs with
  `ACCUMULATORS` independent lanes, and planes, lanes and table are aligned to `VECTOR_BYTES` with rows
  padded in zeros (`LANES_PER_VECTOR`) so no scalar tail is left. The lanes pass through `lanes` before
  being summed, since summed in registers LLVM's SLP vectoriser paired the two channels instead of
  packing each channel's lanes.
- **The dither stage has a flat path and a shaped one, works in f64, and the plan's curve picks.**
  `Dither::prepare` resolves `NoiseShaping::at` the output rate once and keeps what survives as the
  stage's `applied`; `NoiseShaping::None` requantises with no error history and allocates none. A sample
  is worked in steps of the target grid in f64, where every grid value up to 32 bits is exact (in f32 the
  noise near full scale was itself rounded, biasing the result), so a 32-bit target is dithered like any
  other and `Dither::new` cannot fail; an integer source an S32 word holds whole is still `Repacked`. The
  output is clamped to `[-1, 1 - step]` but the error fed back is taken before the clamp, and an error
  that is not finite or lies past what the dither can make (`DitherKind::largest_error_steps`) is fed
  back as nothing, since one NaN or wild sample would ride the feedback for the rest of the track. The
  shaped path is one function generic over the tap count, each channel's errors held twice over in a ring
  twice the tap count long so the recent errors are one contiguous window, and the match is on the enum
  so a curve added to `NoiseShaping` must say which path it takes.
- **Lipshitz runs only at the rates it was designed for; Threshold is designed at the rate it runs at.**
  The Lipshitz coefficients are an E-weighted fit at 44.1 kHz, so `NoiseShaping::at` resolves it to flat
  at every rate but 44.1 and 48 kHz. `Threshold` is an error-feedback filter `Dither::prepare` designs
  for the output rate from Terhardt's (1979) threshold in quiet (held at its 1 kHz value below 1 kHz,
  clamped to `THRESHOLD_RANGE_DB`), an order-12 prediction-error filter by Levinson–Durbin whose noise
  transfer function is `A(z)` itself, minimum phase by construction. `plan_for` stores what survives in
  `OutputPlan::shaping` and `resonate explain` prints it, so a Lipshitz fallback to flat is visible.
  `EngineConfig` defaults to `Threshold`, so a 16-bit device at any rate gets shaped dither out of the
  box, as does a 24-bit device behind a volume below full.
- **Digital silence held a moment is handed on as digital silence.** Dither decorrelates the error of a
  signal the grid cannot hold, and a run of exact zeros has none: dithered, a gap reached a 16-bit device
  as shaped hiss a DAC's silence detection could never see. Once every channel has read zero (or under
  `SILENCE_FLOOR`, where an equaliser's filter tail rings) for `SILENT_FOR_BEFORE_MUTING_SECONDS`, the
  stage writes zeros and clears the error it feeds back, and the first non-zero frame is dithered again
  from a clean history; a fade is never taken for silence.
- **`OutputMode` names what the plan does to the signal, and a shorter word is not a resample.** Four
  readings, decided in `plan_for` and drawn as the chip on the playback bar and inspector: `BitPerfect`
  (the stream is the source's own triple), `Repacked` (the target holds every source value losslessly,
  only the container wider), `Dithered` (rate and channel map the source's, only the word shorter,
  dither covering the difference) and `Converted` for all else, including a narrowing with dither off.
  `no_convert` is `mode != Converted`. The order is load-bearing: `dithers` is resolved *before* the
  mode, since a narrowing the listener switched dither off for is a truncation and must not answer to a
  name saying it was covered.
- **The channel layout is negotiated like rate and format, and one matrix is the whole downmix.**
  `plan_output` asks `SinkInfo::best_spec_for` for rate, format *and* layout together, so the plan can
  never name a triple the sink did not advertise as one (`crates/resonate-pipewire/src/sink.rs` asserts
  it over the cross product). The fit is lexicographic over rate, then channels, then format: keeping
  every channel beats keeping depth. `resonate-dsp`'s `Remix` runs first, so resampler and dither cost
  the sink's channels. Its matrix is built from `ChannelPosition`: a channel the target also has is
  copied at unity (`UNITY`), one it lacks folds into its nearest neighbours at −3 dB (`MINUS_3_DB`), the
  LFE is dropped, not folded, and a target channel nothing feeds stays silent. Routing is by position,
  never index. Each row is scaled so its gains sum to at most one, so no downmix can clip a full-scale
  source: attenuate rather than clip.
- **The chain is carried in f64 from the decoded word to the sink's.** `Processor::process` and `flush`
  take `f64` slices; the engine widens exactly what the decoder handed it into `Output::widened` through
  `SampleData::widen_into` before the chain, and narrows once, into the sink's word, in `Output::stage`.
  An f32 carrier held 24 bits of mantissa, so a 32-bit integer source lost its low byte on the way in.
  `ChainBuilder::input` still names the carrier `SampleFormat::F32`, the spec being a negotiation
  vocabulary with no wider float; the stages read only its rate and channels.
- **Lossy restoration is a spectral stage for MP3, AAC and Vorbis alone, off unless asked.**
  `Restoration` is `Off`, `Repair` or `Extend` (the `restore-lossy` key, marked experimental), and
  `Restore` is pushed after the remix and before the resampler, working at the source rate where the
  encoder cut. `Decoded` carries the source's `Tuning` (the engine maps `Codec` onto one in `tuning_of`,
  naming the lossy codecs rather than trusting `is_lossless`, which calls `Unknown` lossy too) and the
  lowpass wall the study found, off `TrackHints`, else found as the music plays; a lossless source never
  gets the stage, and one that does makes the plan `Converted`. It is a weighted overlap-add that holds
  back its first `frame − hop` frames and hands them on in its flush, so a track keeps its length.
  **Repair** lifts the droop an encoder's lowpass leaves under its wall and fills holes with noise;
  **Extend** also rebuilds the band from the wall to 97 % of Nyquist from the same width below it,
  never louder than 3 dB under its own source. The codec's curves live in `Tuning` and are applied in
  the spectrum, not handed to the equaliser. Its latency is flushed before a swap as the guard's is.
- **A `Chain` carries a channel width per stage, not one for the chain**, since `Remix` changes the
  frame width mid-chain: the builder records `output_spec(spec).channel_count()` per stage and sizes the
  scratch by the widest *sample* count. **A chain drains in one flush, and a flush with less room is
  refused:** the builder sums what every stage can still hand on into `Chain::max_flush_frames` and
  `Chain::flush` answers `Error::OutputTooSmall` for a shorter destination; one flush is the whole tail
  on purpose (a resampler pads half its filter, keeping a track exactly its length), and
  `Engine::flush` sizes the carrier to `max_output_frames`.
- **Clip prevention attenuates where it knows the peak; only an over it could not see is ridden down.**
  A ReplayGain boost is capped where the track's declared peak would reach full scale. `AppliedGain` in
  `resonate-core` pairs the gain with the peak the mode selected and owns that arithmetic, so the DSP
  stage, `resonate info` and the inspector cannot disagree. `AppliedGain::heeding` folds a measured true
  peak in beside the declared one and keeps the larger, so a track whose study says it passes full scale
  between samples is turned down by exactly that even with ReplayGain off (`true-peak` off leaves the
  declared peak alone). The per-sample clamp survives only where a *boost* has no peak and no guard
  follows; with `true-peak` on, `gain_of` sets `GainConfig::guarded_after` and the overs reach the guard
  whole. At unity or below the guard answers, so `GainConfig::limits_every_sample` reads the amplitude as
  well as the peak. A boost capped at exactly unity changes no sample, so `AppliedGain::adjusts` answers
  false and the track plays bit-perfect.
- **What the catalog studied reaches the player through `Hinting`, beside `StandIn` and not gated on the
  vault.** `resonate-core::TrackHints` (measured true peak and lowpass wall) is core vocabulary since the
  codec's `Sources`, the library and the engine all need it. `Library::hinting` answers from
  `track_studies` by the row's path and span; `Unwrapped::open` asks once and folds the answer into the
  gain in `levelled`, which `re_level` and `Command::SetTruePeak` run again.
- **A track nobody studied is measured where its peak would matter.** Where `true-peak` is on, the gain
  in force has no peak (neither tag nor study) and either asks for a boost or the source is float,
  `measure::Measuring` decodes the row whole on a `resonate-peak` thread through a `TruePeakMeter`, and
  each run-loop pass asks whether it landed: `Track::heed_what_was_measured` puts the peak into the
  hints, `levelled` runs again and `retune` reshapes the chain in place. Dropping the track drops the
  `Measuring`. `Track::peak` is a `Peak` (`Unasked`, `Measuring`, `Unmeasurable`) so a row that would not
  decode to its end is not decoded again for every settings change. The measurement is not kept; the
  lookup's study is what the catalog keeps.
- **The true-peak guard is a lookahead gain, not a clipper, and touches nothing under the ceiling.**
  `resonate-dsp`'s `TruePeak` is pushed after the gain stage and before the dither wherever `true-peak`
  is on and the plan converts, never on a bit-perfect or repacked stream. It reads each frame at eight
  times the rate and wherever a phase passes −0.1 dBTP asks for the gain bringing it under; the smallest
  asked across the lookahead is averaged over the same span, so the gain has fallen by the peak's
  frame. A stream never passing the ceiling is multiplied by exactly one and comes out bit for bit, only
  later: the audio waits `lookahead + 48` (`REACH`) frames, held back and handed on in its flush so a
  track keeps its length, and `Engine::swap_chain` first flushes the running chain's held tail into the
  ring (`Output::hand_on_what_the_chain_holds`). The interpolator is 96 taps because every fractional
  phase must pass a tone near Nyquist at its own level, inside the 0.1 dB between ceiling and full scale;
  the study's `TruePeakMeter` uses the same one. The guard skips the interpolation wherever nothing can
  be over (no phase answers more than the loudest sample times the largest sum of absolute weights, 3.0:
  `quiet_below`, `loudest_that_could_pass`), and limiting is constant time.
- **The pre-amp and an untagged track's gain are part of what ReplayGain asks for, so clip prevention
  weighs them too.** `Levelling` is a `Trim` for each (quantised millibels, so `OutputSettings` keeps its
  `Eq`) and `resolve_replay_gain` folds them into its `AppliedGain`: a tagged gain is raised by the
  pre-amp and keeps its tagged peak; a track declaring no gain takes `untagged` with no peak, the one
  case the per-sample limiter is for. **A track declaring no gain that the catalog studied is levelled by
  what was measured:** `TrackHints::measured` is a `MeasuredGain` (ReplayGain 2.0's −18 LUFS less the
  study's integrated loudness; the album's from the energy mean of every track's loudness, only where
  every track of the album has been studied). `engine::tagged_or_measured` uses it only where the tags
  carry neither gain, so a tagged file is never second-guessed; it gives a delivered row a gain, the
  vault having stripped its tags. The keys are `replay-gain-pre-amp` and `replay-gain-untagged`.
- **The plan is the one judge of a gain stage, a converting plan always carries one, and the engine picks
  its fill branch from the plan.** `gain_config` answers `None` at full volume under unity gain, and
  `plan_for` asks for a stage anyway wherever the plan resamples, remixes or equalises (such a chain is
  not bit-perfect whatever its gain, and a stage already there makes every volume and ReplayGain change
  a retune rather than a change of shape). Only a plan with no other stage leaves it out, and
  `dop_survives` still reads `gain_config`, so DoP is refused only where a gain would change samples.
  `GainStage::is_transparent` answers false, so the builder keeps every gain stage the plan pushes.
  `Engine::fill` and `Engine::flush` pick the byte copy or the conversion from
  `OutputPlan::is_transparent`, the reading `delivery()` asks the decoder's format from.
- **Every float reaching an integer word is rounded to nearest, ties to even, and saturated to the
  format's range, and one set of conversions in core is the whole of how.** `SampleData::write_f64` is
  what `Output::stage` narrows the carrier through on its way to the ring, and `SampleData::write_f32`
  its narrow twin that `convert_into` and `retype` and the DSD decimator use. A narrowing nothing
  dithers (dither off, or a float source reaching an integer word at its own rate) is
  `OutputPlan::rounds`, making the plan non-transparent with an empty chain, so the samples reach
  `write_f64` rather than symphonia's flooring shift or a truncating cast (which leaves a dead band a
  step wide around zero). The rounding adds a constant big enough that IEEE rounds the sum to a whole
  number, half to even, rather than calling `round_ties_even`: the `x86-64` baseline has no `roundps`
  and the addition vectorises on SSE2.
## The sink

- **A sink's formats are read again whenever the node says they moved.** A port switched or an EDID read
  again can change what a node advertises while the node stays, so its `info` event is watched for
  `PARAMS`: the `EnumFormat` entry's `SERIAL` flag flips on every change, `SinkRecord::formats_moved`
  compares it with the last seen, drops every index held and asks for `EnumFormat` again, and
  `SinkChange::Reformatted` has the engine survey the graph afresh, issued after the enumeration.
- **A global leaving the registry takes its proxy with it, whatever it was told first**, so a card
  unplugged before its routes were enumerated does not keep a proxy and listener until the client
  reconnects.
- **The client outlives its daemon.** Everything a connection holds (core, registry, listeners, bound
  proxies) is one `Graph`, and `Reaching` makes one: the loop keeps its main loop and context for the
  process's life and connects a core through them as often as it must. The core's `error` event with a
  broken pipe, a reset, an abort or a missing connection is the daemon gone; it sends `Request::Lost` through the loop's own channel (a connection
  cannot be torn down inside its own callback). `Lost` drops the streams and the graph, answers every
  pending `Sync` by dropping it, empties `Discovered` and announces each known sink removed, and a thread
  sends `Request::Reconnect` a second later, again until a core connects, **the first connect being one
  more of these tries**. **A daemon that stops answering without closing is lost too:** a loop timer
  sends `Request::Heartbeat` every `HEARTBEAT_EVERY`, which sends the core a `sync` and keeps it as a
  `Beat`; a beat still unanswered after `DAEMON_ANSWERS_WITHIN` goes through `lose_the_graph`, the
  same path a broken pipe takes, and the reconnect thread tries again until the daemon answers
  (`a_client_whose_daemon_stops_answering_takes_it_as_gone_and_finds_it_again`, which `SIGSTOP`s its
  hosted daemon). **Nothing inside the request callback sends on the request channel:** pipewire-rs
  runs the callback under the channel's own lock, so a send from there never returns. While there is none, a `Sync`, an open and a capture answer
  `Error::Disconnected` at once rather than timing out as `LoopStopped` (`PipeWire::unanswered`).
  `tests/reconnect.rs` reruns its binary under `PIPEWIRE_RUNTIME_DIR` with a `pipewire` of its own.
  **A recording carries on across the restart:** `resonate-listen`'s `Capture` reads its disconnected
  events channel as lost and asks the same client for the same capture every `LOOKS_EVERY`, recording
  into the same `Recording` from where it had filled.
- **A metadata key cleared is read as unset.** `Discovered::heard` reads the `settings` and `default`
  metadata keyed by which object spoke (`HeldIn`): a key with no value clears what it held, and a key of
  nothing clears every key that object carries and no other's, as `pw-metadata -d` does. The default
  sink announces `SinkChange::DefaultChanged` only where the name it reads moved. **Only the core
  subject's keys are read** (subject 0, `CORE_ID`): the `default` metadata also carries per-node keys
  (WirePlumber's `target.object`), cleared with a key of nothing when the node leaves, and reading that
  as every key cleared wiped the default sink and rebound the engine to the first sink.
  **A metadata object leaving the registry is the same clear**: `Metadatas` keeps each proxy with the
  `HeldIn` it was bound for, and `global_remove` drops the proxy and reads a key of nothing for that
  object, so a WirePlumber restart leaves no value standing from an object that is gone
  (`a_metadata_object_that_leaves_takes_the_values_it_held_with_it`).
- **The chosen sink is named, not numbered.** `EngineConfig::sink` and `Command::SetSink` carry a
  `NodeName`, which `select_sink` matches on every stream open, since a PipeWire id is assigned per
  object and a replugged device gets a new one. `OutputStatus::sink` stays a `SinkId`, reporting what
  was opened. A name nothing answers to warns and falls back to the default until the device turns up.
- **Whether a sink is hardware is a question about the `Device` above it, not the node.** A `Node`
  global carries `device.id` and not `device.api`. `Discovered::driven` is the set of device ids whose
  global names a driver, `SinkRecord::device` the id the node points at, and `Discovered::is_hardware`
  answers for the pair at snapshot time, so the two globals may arrive in either order
  (`resonate sinks` *DRIVEN BY*).
- **Which physical port a sink comes out of is the `Device`'s to answer, and the seat is the join.** A
  card's `Route` params name its ports; a sink node's `card.profile.device` names the seat on that card,
  and `DevicePorts::serving` is the pair. The seat is read off the node's `info` event, only where the
  change mask says `PROPS` (a volume change raises an `info` with no props, which must not read as *no
  seat*). `Route` is the port the card is *switched to* and outranks the rest (`Held::Current`);
  `EnumRoute` is every port it offers (`Held::Offered`), the only place an unplugged port appears.
  `Plugged` is `Yes`, `No` or `Unsaid` where the driver does no jack detection.
- **The card says which output it is switched to, and the port whose volume it is.** The `Device`
  subscribes to `Profile` beside its two route params, and `parse_profile` reads the current one into
  `Discovered::profiles`; `CardProfile` is its `ProfileIndex` and description alone (no switching is
  offered), and a moved profile announces `SinkChange::Switched`. `HardwareVolume` is read from
  `route.hw-volume` in `SPA_PARAM_ROUTE_info`: `Yes`, `No`, or `Unsaid`, never drawn as `No`.
- **A device turning its own volume can be handed the slider, and the stream then stays bit-perfect at
  any volume.** `device-volume` (off by default) is `EngineConfig::device_volume`,
  `Command::SetDeviceVolume` and `OutputSettings::device_volume`. `Attenuator::of` weighs it against
  `SinkInfo::turns_its_own_volume` (the route saying `route.hw-volume` is `Yes`, never `Unsaid`) and
  `Attenuator::leaves` is the volume the stream still applies: `Volume::MAX` where the device takes it,
  so the plan keeps no gain stage for it while a ReplayGain adjustment stays the stream's own. The
  device's volume is its route: `PipeWire::set_device_volume` sets `Route` on the `Device` with its
  loudest channel at `Volume::to_gain` and every other scaled by the same factor, `save`d (what
  pipewire-pulse writes for a desktop slider; the node's `Props` would be the adapter's software
  volume), **keeping the device's balance and its mute**; `Command::SetDeviceMute` reaches
  `PipeWire::set_device_mute`. **The slider starts where the device already is** (`Volume::heard_at` of
  the route's reading), so handing it over cannot jump headphones to full, and it follows a device
  turned from the desktop: a `Route` is not re-sent when its volume moves, so the device's `info` saying
  `PARAMS` changed enumerates it again, `SinkChange::Turned` names the sinks and the survey ends in
  `follow_the_devices_volume`. What the engine sent comes back the same way, so `Engine::turned`
  remembers the last `TURNS_REMEMBERED` gains and a reading within `ONE_LEVEL_WITHIN` of any is an echo.
  **A device muted from the desktop is kept apart from its level:** `OutputStatus::device_muted` is
  `Attenuator::hears_the_mute_of` the bound sink, and `DevicePorts::keep` counts a mute that moved as a
  turn; the window's mute mark sends `SetDeviceMute(false)`.
- **A stream's reported latency is counted in its own frames, converted where the graph's tick rate is
  known.** `pw_time.delay` is in `pw_time.rate`, the graph's clock, not the stream's. The `process`
  callback holds the only consistent snapshot of delay and rate, so it builds a `GraphTime` and
  publishes `downstream(spec.rate)` as one `AtomicU64` of stream frames (two atomics would tear and
  converting later takes a lock the RT thread may not). `SinkStream::latency` is `Frames`.
  `GraphTime::buffered` is already at the stream's rate and so added, not converted.
- **Bit-perfect is what the graph runs at, read every cycle, not only what the stream was opened at.**
  Another client can hold the graph at a second rate after the stream opened, and the graph then
  converts the stream however the plan reads. `StreamClock` is what the callback writes each cycle
  (the latency and, where a tick is one frame, `GraphTime::graph_rate`, both atomics) and `SinkStream`
  reads; `poll_stream` copies the rate onto `OutputStatus::graph_rate` and `OutputStatus::hears`
  demotes the published `mode` to `Converted` wherever `converted_by_the_graph` (the graph's rate is
  known and is not the negotiated one), the plan's own mode coming back once it is again. The plan is
  left as it was: `no_convert` and every reshape still read `OutputPlan::mode`. The inspector says
  *→ 48 kHz by the graph* and *graph runs at*
  (`a_stream_the_graph_runs_at_another_rate_is_not_called_bit_perfect`).
- **The callback fills the quantum the graph asked for, not the buffer it was handed.**
  `pw_buffer.requested` is that quantum in frames and `Cycle::asked_for` turns it into bytes, clamped to
  the room the pool gave and falling back to all of it where the graph names nothing (also where the
  packed DoP path lands). Filling the pool's whole buffer drained the ring many quanta at a time and
  declared an underrun against that much want.
- **Silence about a capability is not a refusal.** A node answering `EnumFormat` with no channels
  property, or no formats at all, is believed about what it said and left alone about the rest: the
  candidate list falls back to the source's own layout, or `allowed_rates` crossed with the source's
  format. `SinkInfo::supports` stays strict (this exact format at this exact rate; the DoP path,
  `resonate explain` and the settings pane read it), but a format entry naming no channels takes any
  layout (`SinkFormats::takes`).
- **A sink's advertised formats and the graph's `allowed_rates` are separate fields.** A device
  advertising 192 kHz is irrelevant if the daemon will not switch the graph to it; conflating them
  would make the bit-perfect claim unfalsifiable.
- **Twenty-four bits are one depth and two words, and which word goes on the wire is the graph
  boundary's alone.** `SampleFormat::S24` is sign-extended in the low 24 bits of an `i32`, four bytes
  everywhere in the workspace, while a device may want three. `format::WireWord` in `resonate-pipewire`
  is that difference: `sample_format` reads `S24LE` and `S24_32LE` as one depth and `spa_format` takes
  the word as a second argument. `SinkFormats::words` is a `Words` so `resonate sinks` and `explain` say
  which were named. `build_stream` offers both as two `EnumFormat` params, the packed one first (a
  24-bit DAC's default is usually the packed word, and a conversion avoided is the point of the
  bit-perfect path). What the graph chose comes back through `param_changed`, stored on an `AtomicBool`
  the callback reads, sent as `StreamEvent::FormatChanged` and kept on `OutputStatus::words`, which the
  inspector draws as *on the wire*.
- **Packing is done in the graph's own buffer, forward, allocating nothing.** `process::pack` reads the
  low three bytes of the word at `4i` and writes them at `3i`, and `3i + 3 <= 4i + 3`, so the write never
  reaches an unread word and no scratch is needed. The ring still fills at the padded stride and
  `Silence::write` still lays DoP markers as four-byte words; packing keeps bytes 0 to 2 and the marker
  is byte 2, so a DoP carrier survives. Without it a 24-bit master reaching a device advertising
  `S24LE` and `S16LE` alone was dithered to 16 bits.
## Fixtures

- **A test asserting which row plays pauses the transport first.** The fake graph is pulled by the test
  thread alone, but a decode failing on a loaded machine reaches `Engine::fail`, which skips. `stand_still`
  is the pause; the queue-order tests take it before reading a row and again after any command starting
  the transport.
- **The real-file fixtures are built by ffmpeg at test time, and the suite skips without it.**
  `crates/resonate-codec/tests/encoded.rs` also builds a rip through `flac`/`metaflac` (the block list
  asserted) and an MP3 through `lame`, each gated on its own tool, and writes one CAF itself (ffmpeg's
  muxer refuses AAC), laying ADTS packets into `desc`, `pakt` and `data` chunks as `afconvert` would.
- **`RESONATE_REAL_FIXTURES` points the suite at a folder of real files.** Every file under it must
  probe, name a container and codec this build knows, report a duration and decode its first block.
  Unset, it prints a skip.
- **The transport and bus tests drive a real `Player` thread and poll for the state they expect**, so a
  state that never arrives costs the full patience before failing; the failure names the transport it
  watched.
