---
paths:
  - "crates/resonate-engine/**/*.rs"
  - "crates/resonate-audio/**/*.rs"
  - "crates/resonate-codec/**/*.rs"
  - "crates/resonate-dsp/**/*.rs"
  - "crates/resonate-pipewire/**/*.rs"
---

# The audio path

Invariants from file to sink. Callback contract: `realtime.md`.

## Decode

- **Depth: container, never decoder buffer.** symphonia leaves ALAC `bits_per_sample` unset;
  `declared_bits` asks each container (ALAC magic cookie, WAV `wValidBitsPerSample`, FLAC
  `STREAMINFO`, Matroska `BitDepth`). Padding lies about the master.
- **Layout named only where the container places every channel as that layout does.**
  `positioned_layout` weighs symphonia's speaker mask against named layouts, else
  `ChannelLayout::Discrete` (a same-count guess lets a downmix drop a 6.0's fourth channel as LFE).
- **Tags are read under the writer's naming, not only the container spec's.** `tags::Naming` decides
  from the key set (one `VORBIS_COMMENT_ONLY` name = Vorbis comments; ffmpeg's Matroska `ALBUM`,
  `DATE` otherwise reached the `TagSet` through nothing). What a reader never exposes is taken off
  the source first (`riff.rs`: WAV `INFO`, `id3 `; `matroska.rs`: segment `Title`); a WAV's `id3 `
  revision outranks `INFO`. Every metadata revision is read (`tags::Revisions`, oldest first, each
  absorbed alone so `Naming` weighs one writer's keys; symphonia `skip_to_latest` drops the rest).
  `Builder::listed` makes a name given twice in a revision every one of them for list fields
  (artist, album artist, genre, credits; `LISTED_APART_BY`, `id3_list`); title, album, id, number:
  last wins.
- **Text blank once trimmed is no tag** (`tags::given`, the one door into a `TagSet` slot, leaves it
  unchanged). ReplayGain values and numbers likewise (unparsable, zero or oversized keeps the earlier
  frame's value); a gain past `PLAUSIBLE_GAIN_DB` (+-64 dB) is no tag (it would reach the gain stage
  as infinity).
- **Gain heard = declared ReplayGain moved to our reference, else Sound Check, else encoder's.**
  `TagSet::replay_gain` stays as written (tag writing spells it back); `TagSet::heard_gain` is what
  engine, `info`, inspector and catalog `rg_*` read. Declared gain is raised by 89 dB less
  `REPLAYGAIN_REFERENCE_LOUDNESS` (`LUFS`/`LKFS` read 107 dB lower; outside 60-120 dB ignored).
  Neither declared: Sound Check (`iTunNORM`), then LAME header gains (`mpa::encoder_gain`, only where
  the tag after `Xing`/`Info` names an encoder in `LAME_ENCODERS`). Tag-declared peaks stand beside a
  fallback's gain.
- **Matroska length: the segment's, counted as well as read** (symphonia leaves `duration` unset).
  `Segment::duration` keeps the declaration while clusters stay within it; the count is a deliberate
  lower bound, read from at most `MAX_ELEMENTS_COUNTED` cluster children in total (past it the tally
  is lost like a walk cut short), and too-long declarations are detectable only for the first
  `A_OPUS`/`A_FLAC`/ `A_VORBIS` track. **Vorbis packet length is named by the previous packet**
  (`vorbis::Windows`). Matroska seeks are approximate (ms ticks).
- **Opus: `opus-rs`, registered beside symphonia's decoders** (symphonia 0.6 demuxes Opus, decodes
  none). **Billed at 48 kHz whatever the container says** (`opus::rate_of`; ffmpeg writes the input's
  rate as Matroska `SamplingFrequency`). `R128_TRACK_GAIN`/`R128_ALBUM_GAIN` read as ReplayGain 5 dB
  louder. **Pre-skip is the container's:** Ogg counts it inside granules, Matroska shifts by
  `CodecDelay`, MP4's edit list is read by the box scan. **Matroska's end is the last block's
  `DiscardPadding`**, kept per `A_OPUS` track (`Segment::discarded_frames(track, rate)`; another
  track's last block neither keeps nor replaces it, so the track the decoder selected is the one
  asked); declared length is counted from each packet's TOC byte (`Segment::opus_music`, for the
  counted track only), trusted only if nothing escaped the count. **Seeks start early** (`PRE_ROLL`, 19 200 frames: CELT
  predicts band energy from the previous frame) and discard those frames. **AAC seeks start
  early too** (`AAC_PRE_ROLL`, four packets: the MDCT overlaps the previous packet, so a landing
  with no lead-in plays a fade-in), and every jump of a spliced M4A is a seek. The roll is taken in
  timestamps (`Timeline::rolled_back`), so it may reach into the priming but never before the
  track's start; a non-sample-accurate timeline rolls back in music frames. An AAC decoder keeps
  small state across `reset`, so a seeked decode agrees with playing through to within about
  -30 dB, not sample for sample.
- **WavPack Hybrid is billed as the lossy codec it decodes as** (`wavpack::HYBRID_CODEC_ID`,
  `Codec::WavPackHybrid`, not `is_lossless`); `wavpack.rs` rewrites packets because
  `symphonia-codec-wavpack` mishandles extended bits of 32-bit/float blocks; `.wvc` files are not
  read. **Vorbis setup headers are walked before symphonia reads them** (an ordered codebook can
  index past its table and abort in release; `vorbis::Vorbis`).
- **Own readers where symphonia has none:** Monkey's Audio (`ape.rs`; markers `MAC ` and `MACF`, else
  the probe opens the stored `RIFF` header as WAVE; 32-bit stereo refused; the last frame ends where
  the audio does, before a trailing APEv2 or ID3v1 tag and the terminating data, as the SDK counts it); WAVE too long for RIFF
  (`wide.rs`, RF64/BW64 `ds64` and Wave64; scan takes `.rf64`, `.w64`).
- **Every PCM symphonia names = `Codec::Pcm`; ADPCM = `Codec::Adpcm`** (lossy). **64-bit float
  decodes as 32-bit float** (`container::representable`); declared depth stays 64.
- **This build drops every priming itself** (`AudioDecoderOptions::gapless(false)`); only
  `MediaInfo::playable` trims. `container::priming` prefers `Track::delay`/`padding`/`num_frames`,
  else the box scan (`iTunSMPB`, else the `elst` entries) or an iTunes MP3 `COMM` note
  (`Priming::behind_a_decoder_delay` adds `MP3_DECODER_DELAY` to delay, subtracts it from padding).
  **`playable` = the first decoded frame heard and how many are heard, possibly open-ended;**
  `duration` = playable count (seek bar, `mpris:length`, `heard.rs`). `Priming` holds the count as
  `Option`: a reader can name a delay without a length and the priming must still go.
- **An edit list plays in the order it lists its edits.** `elst` entries continuing one another in
  media time (within a tick) join; zero-length entries and leading or trailing empty edits are
  dropped. One stretch left is the gapless window as ever (`MediaInfo::splices` `None`). More are
  `Splices` (`Edit::Music` in music frames, counted from the earliest frame any edit reads, which is
  the priming; `Edit::Silence` for an empty edit between music), `playable` lasting their sum. The
  decoder walks them itself (`Decoder::spliced_block`): where an edit does not continue where the
  reader stands it seeks there (a cut skips ahead, a reorder seeks back), an empty edit is handed
  out as silence, and `seek`, `confine` (a cue span over a spliced file) and a settled spool place a
  heard frame through `Splices::place_of`. The list is read once at open into one `Arc<[Edit]>`;
  walking it allocates nothing. A source that cannot seek skips ahead to a cut and leaves out an edit
  reaching back (it cannot go back; the rest of the list plays on, so the track runs shorter than
  its `playable`). Splices reaching past the samples `stts` holds play the first stretch alone.
  **The list is kept in the media's ticks and rescaled to the decoder's rate on asking**
  (`Edited::priming_at`), so a track whose `mdhd` timescale is not its sample rate (HE-AAC's core
  rate) is still honoured; only an `iTunSMPB` declaration, which counts output frames, is declined
  where the timescale disagrees. **The whole `elst` is read** (the box up to `MAX_EDIT_LIST_BYTES`,
  1 MiB; a count the box cannot hold declines the list rather than reading a prefix). **An edit's
  media rate is honoured: a dwell (rate 0) holds one frame, which for audio is no sound, so it is
  an empty edit** (silence for its segment, and dropped where leading or trailing); **any other
  non-unity rate declines the whole list**, since the player has no varispeed and a window claimed
  at normal speed would be wrong.
  **Fragmented MP4 length is in the fragments, not `moov`** (`mvex/mehd`, else the top-level `sidx`
  boxes added up, one a DASH segment, those naming the sound track's id or else the first one's:
  `Movie::fragmented_length`; the walk goes on past `MAX_CHILD_BOXES` top-level boxes once a `sidx`
  is seen, `MAX_INDEX_ROUNDS` times).
- **MP3 naming no length is counted where bitrate moves.** symphonia reckons a stream with no
  Xing/Info count and no VBRI from its first sixteen frames (off by half for VBR);
  `mpa::counted_frames` (`Prescan::buffered`, whole sources only) walks every frame header (at most
  `MOST_FRAMES_WALKED`, past which the reader's reckoning stands), leaving streams whose header
  names frames, and constant rate, to the reader. A seek past the reader's shorter reckoning lands
  on its last frame and decodes on. **A walk is done once, by the scan, and playback is told its
  result** (`Walked`, on `MediaInfo::walked`): the frame count above and the Matroska cluster tally
  (counted length, discard padding, Opus and framed sample counts, the counted track) are what
  `Prescan::buffered` found, written as one text (`Walked::written`) that `Sources` hands back through
  `Hinting::walked` at every open but the scan's (`probe_scanned` opens afresh, so a revised walk is
  never fed its own old answer). A hint is believed only for a source of the byte length it was read
  at (`Walked::size`; the library gives its row's `file_size`): a file another program rewrote since
  the scan is walked again. A source whose provider hints nothing (the phones) walks as before
  (`a_walk_read_before_spares_the_frame_walk_of_a_file_of_the_same_size`,
  `a_walk_read_of_a_file_of_another_size_is_not_believed`,
  `a_count_already_known_stands_in_for_the_walk_of_the_clusters`).
- **MPEG audio and ADTS seek from marks, not from the first frame** (`framed::Framed` wraps
  symphonia's `MpaReader` and `AdtsReader`). A seek walks frame headers itself (`Framing::walk`) from
  the latest of: a mark at least `Framing::LEAD` short of the target (MPEG: room for the bit
  reservoir), the reader's position, the first music frame. Marks are kept every
  `MARKED_EVERY_FRAMES` (32) from walked and heard frames; where a seek finds the reader elsewhere
  than the summed packet lengths say, every heard mark since the last exact position goes. ADTS is
  held to a drift bound (symphonia's AAC keeps state across `reset`), MP3 to equality.
- **`Timeline` holds where the music starts, not the priming length:** one `music_at: Timestamp`,
  zero where the reader named the delay (negative PTS), else `Track::start_ts` plus scanned priming
  (Ogg sets `start_ts` to `-delay`). A seek can land ahead of it (`short_of_the_music`); on a
  non-sample-accurate timeline (Matroska) a landing on the first packet drops `MediaInfo::priming`
  whole.
- **A picture is found once, copied once; the caller says who copies.** `probe_pictured` is the one
  open answering tags and picture together (`Picturing`: `Whether` copies nothing, `Copied` answers
  bytes; a row the sources stand in for answers `Pictured::StoodIn`). Library scan uses
  `probe_scanned` (`library.md`) and digests the first `PACKETS_DIGESTED` (48) packets into a
  `PacketDigest` on the same open (identifies a moved file once retagged). `TagSource::read` takes
  the `Picturing`, so no implementation reads tags and picture through separate opens. The engine's
  `catalog::whole` probes under `Copied` and files bytes where the picture is waited on, else into
  `Spares` (outside `ART_BYTES_HELD` until drawn). `MAX_COVER_BYTES` (24 MiB) is checked against
  `data.len()` before `to_vec` (oversized visuals are declined, not read failures); `choose` prefers
  front cover, then `Other`/untyped, never icon, leaflet, back cover, disc, portrait.
- **Engine catalog follows the file.** Each entry keeps file size+mtime (`Stamp`), weighed at most
  every `LOOKED_AT_THE_FILE_EVERY` by the reader thread (queued by the lookup, `Held::to_look`, after
  the lock is released); a changed file is forgotten and the revision bumped; a failed read
  (`Look::Failed`) is reclaimed after `READ_AGAIN_AFTER`. Nothing stats under the lock or on the
  caller's thread: a stalled mount stalls the reader alone.
- **Hand-rolled parsers never allocate what a header declares:** `riff.rs`, `matroska.rs` read
  `declared.min(MAX_…_BYTES)` (`id3 ` chunk via a `take` growing only as bytes arrive), then seek
  absolutely past the claimed element. What can crash symphonia is guarded where the prescan sees
  it: `container::open` refuses more channels than a WAVE layout holds (`Error::TooManyChannels`) or
  a mask `riff::a_mask_the_decoder_cannot_widen` (`Error::ChannelMaskNotRepresentable`); `caf.rs`
  checks the CAF reader's declared values first (`Error::PacketTooLarge` and its siblings).
- **Non-seekable sources are spooled; only one too long to spool is read from its head.**
  `container::open` reads it into memory up to `SPOOLED_AT_MOST` (256 MiB) for at most
  `SPOOLED_WITHIN` (750 ms); ending inside both, it opens seekable. A stream still arriving goes on
  onto disc: `spool::Spool` = unnamed unlinked file under `TMPDIR` else `/var/tmp` (`/tmp` is tmpfs
  on Arch, Fedora, Flatpak), copied by a `resonate-spool` thread up to `SPOOLED_ON_DISC_AT_MOST`
  (8 GiB) or the room `fstatvfs` finds less `LEFT_FREE_ON_DISC` (256 MiB), whichever is less: a
  source whose length is known and does not fit is refused before a byte is written
  (`spool::Refusal::NoRoom`; the stream plays from its head, a DSD or Monkey's Audio file that
  must be whole is `Error::NoRoomToSpool`) (`a_source_longer_than_the_disc_has_room_for_is_refused_before_anything_is_written`); symphonia gets the `Spooling` side (`is_seekable() == false`). **Once all has arrived it
  can seek:** `Decoder::settle_the_spool` reopens the file as seekable `Unspooled` at the old
  decoder's frame; the engine asks every pass (`Engine::settle_what_was_spooled`) and republishes the
  digest so MPRIS `CanSeek` follows. One cut short never claims to be whole. DSF, DSDIFF and
  Monkey's Audio are read by seeking, so one too long for memory is spooled whole on disc with the
  open waiting (`spooled_whole_on_disc`); only where that cannot be done is it
  `Error::ReadBySeeking`.
- **Until it can seek, a track keeps its stream.** A rebuild discards ring and carry and seeks the
  decoder back, impossible for such a track. `Engine::seek` refuses with `codec::Error::NotSeekable`
  (`Cause::CannotSeek`) except a seek to its start; a stream-rebuilding setting goes through
  `rebind_where_it_stands`, owed until `settle_what_was_spooled` pays it. Graph-forced rebuilds
  (device gone, renegotiation, stalled discard) still happen at once.
- **A cue track is a window on a file; the window lives in the decoder.** `Decoder::open_span` seeks
  to the span's first frame, stops at its last, rewrites `MediaInfo::duration` to the span length, so
  upstream sees a short file; `self.position` stays absolute, only `position()`/`seek()` convert.
  `probe_span` = `probe` through the same cut. A sheet's 1/75 s unit is a `sector`, never a frame
  (`CueStamp::at` is the one conversion). `confine` writes the row's own `TagSet` over the file's
  where a cut names the span's start frame (`CueFile::cut_at`; scan rows use the same call).
  `cue::cut_for` finds the cut: the file's embedded `MediaInfo::cue`, else the sheet beside a local
  file (`<stem>.cue`, then `<name>.cue`) whose `FILE` line names it as scan matching does
  (`library.md`) or whose only cut it is, else a sheet in one of the `DEEPEST_FOLDER_A_SHEET_NAMES`
  folders above (`cue::above`).
- **Track start is a type:** `CueTrack::start` is a `CueStart`, `Written(CueStamp)` for text sheets,
  `Sampled(Frames)` for a FLAC `CUESHEET` block (never rounded to a sector). `MediaInfo::cue` =
  Vorbis `CUESHEET` comment, else the binary block `flac.rs` walks, else chapters
  (`container::chaptered`; two or more distinct starts). **Matroska chapters are read by the
  prescan, hidden from symphonia**, whose reader refuses an `EditionEntry` without `EditionUID`
  (omitted by ffmpeg). The lead-out is `CueTrackKind::Data`: `audio_tracks` skips it, `span_of` reads
  its offset as the last audio track's end.
- **Sheet edge cases, settled once.** A track belongs to the file its `INDEX 01` is in, whichever
  `FILE` line its `TRACK` followed (EAC gaps-appended). A pregap inside one file heads the track it
  leads into (`CueTrack::heard_from`; `PREGAP` is not read). A track whose `INDEX 01` does not read,
  or that starts before the one ahead of it in its file, is dropped. A `FILE` line may name a folder
  below its sheet (`cue::folder_named`). A sheet is read as far as it parses and never fails (only the
  source and `LARGEST_CUE_SHEET` raise a `codec::Error`); text goes through `resonate_core::text`
  (`rust-style.md`) and a renamed sheet is written back in the code page it was read in.

## DSD

- **Carrier rate is what the workspace sees, DoP or decimated.** `SampleRate` caps at 768 kHz
  (`MAX_HZ`), so `DsdRate` holds the native rate and `carrier()` = `hz / 16`; DSD64 = 176.4 kHz
  `S24` stereo either way and nothing downstream needs a special case. DSD512's carrier is out of
  range: `RateNotRepresentable` at open.
- **`Packing` is the guard.** On `MediaInfo` and `OutputPlan`; `untouched`, sole constructor of
  `DopMarked`, writes `resample`, `equalisation`, `gain`, `dither_to`, `shaping` as literals: no path
  from packed source to staged plan
  (`no_setting_the_pane_offers_can_produce_a_marked_plan_with_a_stage_in_it`).
- **DoP is opt-in, off by default** (`EngineConfig::dop`): DoP into a DAC that does not decode it is
  full-scale white noise. Gate: `SinkInfo::supports` exactly, never `best_spec_for` (its
  widest-format fallback truncates DoP to S16 noise; layout fold rewrites markers).
  `Decoder::set_output_format` = samples in this format, clears packing; DoP only via `deliver`.
- **All-zero DSD byte is negative full scale, not silence.** DSF final-block padding is truncated
  against the declared sample count; the decimator FIR history is primed with `DSD_SILENCE` (`0x69`);
  a seek gap is filled from `Packing::silence`, not PCM zeros.
- **The ring is told what wire silence is.** `ring` takes a `Silence` (`Unmarked`, or `Marked`:
  four-byte word + alternate) so the DoP marker keeps alternating through a gap; sole constructor
  `Packing::silence`. `Silence` is `Copy` and allocation-free (`RingConsumer::fill` is the RT
  thread). **A gap is a gap however opened:** a ring dry mid-track would lose a DoP DAC's lock, so
  `fill` pads the rest of the quantum after an underrun, except at a track's end
  (`Discard::finished`). The marker alternates by the decoder's absolute frame, `Silence` by its own
  count, and is read off audio by sign (`dop::packed`); both gap edges are exact (`Silence::follows`,
  `RingConsumer::realigned`).
- **Starve cost is counted in frames, apart from faults:** the ring adds frames asked for and not
  handed to one `AtomicU64` (never for a track's ending short chunk), `RingMonitor::went_without`
  swaps it out, added before the fault is raised. `Event::Underrun` carries frames missed since the
  last one told; exact where the fault queue is not (no room: bare count in `RtFault::Dropped`).
- **Decimation is at the modulator's level unless PCM's is asked for.** DSD full scale = 50 %
  modulation, so a master decimates ~6 dB quieter than the same PCM music. `EngineConfig::dsd_like_pcm`
  (off): `plan_for` adds `DSD_MODULATION_DB` to gain wherever it decimates, capped at a known peak,
  ridden by the true-peak guard where none known. A DoP plan is never raised.
- **Decimation is a conversion; DoP returns once nothing blocks it.** `plan_for` reads the source's
  `Packing`, not its spec: `DopMarked` delivered as samples = `OutputMode::Converted`, no
  `NO_CONVERT`. `retune` asks `packs_again` and rebinds into the marked plan where `dop_survives`.
- **DST-compressed DSDIFF unpacks into the stream an uncompressed one would be** (`dsd::unpacked`,
  `dst::Unpacked`; `FRTE` = frame count, absent: `DsdChunkMissing`). **Frame index found as
  reached:** `dst::Walk` resumes over `DSTF` headers only as far as a read or seek asks. `dst.rs` =
  ISO/IEC 14496-3 subpart 10 as FFmpeg reads it; tests carry their own encoder.
  **A DST frame that will not unpack, or is longer than `PACKED_FRAME_AT_MOST_RAW_FRAMES` (8) raw
  frames (`longest_packed_frame`, never read into memory), plays as one frame of `DSD_SILENCE`**;
  the DSD path counts no holes.
- **An undecodable packet plays as the silence it would have lasted** (`DecodeError` gives `fill` the
  packet's length via `PacketSpan`; one declaring none lasts as long as the last that decoded,
  `Coded::last_lasted`), handed out at most `SILENCE_HANDED_OUT_AT_ONCE` (8 192) frames a block.
  **Holes are counted; all-holes streams refused:** `SILENT_PACKETS_BEFORE_REFUSING` (64) failures
  with no packet ever decoded = `Error::PacketUndecodable`. `Decoder::refuse_holes` makes the first
  hole that error: the vault calls it on what it keeps, so a damaged source is refused
  `NotValidated`.
- **A DSD file's tags are read whole** (ID3 to its declared length up to `MAX_METADATA_BYTES`: a flat
  cut made symphonia refuse a tag with a large cover; DSDIFF `DIIN` fills only what ID3 left empty).
  **Channels are placed where the file says, never by count** (DSF `fmt ` channel type, DSDIFF
  `CHNL` ids; `dsd::placed`; disagreement gives an unnamed layout by count). **DSF bit-reverses, DFF
  does not** (DSF `fmt ` `1` = LSB-first, `8` = MSB-first; wrong order is distorted music, not an
  obvious failure, so a test decodes both orders to the same tone).
- **The decimator is the codec's own** (layering forbids `resonate-dsp`): Kaiser-windowed sinc,
  decimate by 16, 512 taps at DSD64, **kernel growing with rate** (1 024 at DSD128, 2 048 at DSD256,
  `fir_bytes_for`) so the transition band is as narrow in hertz and the audio band as flat. Cutoff
  45 kHz absolute (a box average is the wrong shape: shaped noise rises steeply above 30 kHz),
  >=90 dB at output Nyquist; a DC blocker follows. The filter overshoots a step into all-ones, so
  full scale saturates at 8 388 607 (a 24-bit wire would read 8 388 608 as negative full scale).

## Sources

- **`LocalFiles` is the local provider**, replaced under `SourceId::local()` by the vault's
  `VaultFiles` where a vault is open (`vault.md`); other `MediaLocation`s are refused by name unless
  a provider is registered (seam proved in `crates/resonate-engine/tests/transport.rs`). A provider
  returns a `Read + Seek + Send + Sync` stream and says whether it is seekable; forward-only is
  spooled.
- **A non-filesystem provider is waited on for `Sources::OPENED_WITHIN` only.** `SourceId::local()`
  opens in line (a scan opens hundreds of thousands of files), any other provider on a
  `resonate-open` thread: no answer in time = `Error::OpenTookTooLong`. The abandoned call is not
  stopped, so a provider puts its own deadline on the network. **Reads likewise:** a `Deadlined`
  stream sends every read and seek to a `resonate-read` thread, waited on for
  `Sources::READ_WITHIN`; no answer = `io::ErrorKind::TimedOut`, stream stalled, engine fails the
  track instead of hanging. **A read costs one copy and no allocation once warm**: the stream
  sends its spare buffer with the ask, the thread resizes it (zeroing only growth, up to
  `LARGEST_READ_ASKED`, 1 MiB), reads into it and returns it, and the caller copies the bytes out
  once (the thread may outlive the call, so it never holds the caller's slice)
  (`the_bytes_a_deadlined_stream_serves_are_the_bytes_the_source_held_whatever_the_buffer_asked_with`).
  The local source, vault objects included, is never behind it.
- **Every track opens off the engine thread.** `Engine::start` hands the row to an `Opening` (a
  `resonate-track-open` thread running `Unwrapped::open`) while the engine answers commands
  (play/pause change whether it plays once landed, seek moves where it starts, stop/load/skip drop
  it), publishing `Loading`. **The command that began an opening is answered by its landing**
  (`OPENING_ANSWERED_WITHIN`, or the opening being dropped, answers `Ok`). A failed landing nothing
  waited on is passed over while playing (`Engine::fail`), else `Event::Failed`; thread stopping
  unanswered = `Error::OpenerStopped`. **An opening is published as what it will land as:**
  `Buffering` while the transport means to play, `Paused` where not.
- **A track that cannot seek decodes off the engine thread** (a still-arriving spool or replayed pipe
  waits for bytes with no deadline). `Track::next_block` lends the decoder (`lending::Decoding`) to a
  `resonate-decode` worker; the engine answers `Block::Awaited`, `fill` returns
  `Filled::WhenTheSourceAnswers`, the answer is taken next pass. A delivery set while the decoder is
  away is owed and applied as it comes home. Worker stopping unanswered = `Error::DecoderStopped`.

## Transport

- **Bit-perfect wins at every track boundary; no gapless playback.** A track change drains the ring
  and reopens the stream: once dry, the engine asks the graph to drain and waits for
  `StreamEvent::Drained` (`SinkStream::latency` on the wall clock is the fallback). A stream takes
  every instruction through one `StreamCommand`, keeping that handshake, `set_active` and `close` on
  one ordered channel to the loop thread.
- **Bluetooth headphones can be kept from cutting the start; off unless asked** (they power the radio
  down when nothing is sent). `BluetoothWake` touches only a sink `SinkInfo::is_bluetooth` names: a
  pause fades the ring out instead of standing the stream down (real silence, marked word for DoP,
  until `awake_for` passes); a stream starting on a slept link opens on silence
  (`RingProducer::lead_in`; slept = `Engine::sounded` against `LINK_NAPS_AFTER`, so a track change
  pays no lead).
- **Nothing the listener does cuts the waveform where it stands; it fades over `FADED_OVER`.**
  Deactivating a stream or dropping the ring's contents would click. `Fader` is shared by the ring's
  two ends: the producer asks for silence or sound, the consumer walks its `level` there a frame at a
  time, then feeds silence without consuming (`is_quiet`). The DoP ring is never scaled (a
  multiplied marker is noise) and goes quiet at once on its marked silence. **Pause** stands the
  stream down once the consumer is quiet (`finish_fading`), or after `quiet_within` where the graph
  never pulled. **Stop, track change, every rebind** retire the output (`Engine::retiring`): fade
  out, close once quiet; `promote` opens no new stream until then (the client holds one playback
  stream). **Seek in place** fades old audio out, new in. A stream opened mid-track opens
  `Entering::FadedIn`; at a track's start it opens whole, so a bit-perfect first frame is untouched.
  Only a stream the graph is pulling fades (`Output::is_sounding`).
- **A volume or ReplayGain change is heard at once, on what the ring holds.** The consumer trims each
  frame by level-to-be-heard / level-rendered-at. `RingProducer::hear_at` stores the amplitude to be
  heard; `render_at(amplitude, ahead)` announces the frame from which the chain renders at a new
  amplitude, `ahead` = rendered but not yet in the ring, so the boundary lands on the exact frame.
  The heard level walks to the wanted one over `FADED_OVER`; a frame rendered at the heard level is
  untouched. The gain stage steps; the ring is the only ramp. The DoP ring is never trimmed.
- **A mute is the ring's, not the chain's.** `Command::SetVolume(Volume::MUTE)` over a volume above
  it sets `Engine::muted` and leaves `EngineConfig::volume`, so the chain keeps rendering at the
  volume that comes back. The PCM ring ramps to zeros; the DoP ring feeds marked silence; the stream
  keeps its plan. Unmuting is heard at once, bit-exact. `PlayerState::volume` and the device's turn
  read `Engine::heard_volume`; a device's own reading clears the mute.
- **A seek does not reopen the stream.** rtrb cannot drop what the consumer has not read, so the ring
  carries a discard epoch: the engine bumps it and stops writing, the graph thread drains every slot
  on its next callback and acknowledges, the engine refills; no acknowledgement within
  `DISCARD_TIMEOUT` = stalled, stream rebuilt. A seek while paused reopens instead (a graph not
  calling `process` can never acknowledge); a pause landing after an in-place seek is the same case
  caught late (`Unanswered::NothingPulling`, not a stall). Sink switch and renegotiation still
  reopen.
- **Leaving/returning to unity gain and switching the equaliser reshape the chain in place.**
  `retune` builds the wanted plan against the open stream. `OutputPlan::same_shape_as`: retune the
  chain. Else `OutputPlan::becomes_on_the_same_stream` (same `stream`, `packing`, `remix`,
  `resample`, convolution by `Arc<Impulse>` pointer, and under a resampler or convolver the same
  `restoration`): `build_chain` on the engine thread, swapped in under ring, stream and consumer, so
  nothing reaches the realtime thread. The new chain renders at its own gain from its first frame
  and announces it; dropping the equaliser first eases it out (`finish_reshaping`; `eq.md`).
  **Resampler and convolver are carried across, not rebuilt** (a fresh resampler steps a steady
  level; flushing a convolver put its whole decay, up to `LONGEST_IMPULSE`, ahead of the music):
  where `OutputPlan::carries_the_front_into` holds, `Chain::take_the_front` lifts the stages up to
  the last one `Processor::is_carried_across_a_reshape` names, the rest is flushed into the ring,
  `build_chain_after` builds the stages behind. A restoration switched under a resampler or
  convolver still rebinds; a volume or ReplayGain change never does.
- **A setting asking for the stream already open does not reopen it** (`Engine::keeps_its_stream`:
  what `plan_output` would open now against the open plan's stream, packing and `ring_capacity`;
  `SetBitPerfect`, `SetDop`, `SetBuffer`). **A setting the open plan does not read leaves it
  alone** (`Engine::keeps_its_plan`): quality, filter phase, dither, restoration, noise shaping and
  convolution are weighed by `OutputPlan::renders_alike` (shaping only under a dither), and where
  the chain reads the setting itself when it is built (`ReadBy::reaches_into`: quality and phase under a
  resampler, the dither's kind under a dither) the plan alone cannot say, so it reopens. Choosing
  the device the stream is already on (`stays_on_the_bound_device`, by `chosen_sink`, named or
  followed default alike) reopens nothing. Graph rate is not weighed (`clock.force-rate` pins the
  graph and is set as the stream opens: switching it still reopens).
- **A renegotiation converges because the next plan asks for exactly what the graph answered.**
  `StreamEvent::FormatChanged` carries the settled spec; `downgrade` hands it to `plan_for` as
  target, written into `OutputPlan::stream` whole (writing the source's channels back made the two
  trade turns forever). `MAX_RENEGOTIATIONS` (3) is the belt: a fourth different spec fails the
  track with `Error::Renegotiation`; reset when a track lands
  (`a_graph_answering_another_format_has_the_stream_rebuilt_around_it`,
  `a_graph_that_will_not_settle_on_a_format_fails_the_row_once_the_rebuilds_are_spent`).
- **The run loop waits on every channel that can change its mind; the ring sets the timeout.**
  Wakers: command, sink announcement, survey answer, opening landing, worker's decoded block,
  stream-open answer, stream events. Timeout: half what the ring holds, floored at `SHORTEST_TICK`,
  capped at `PUBLISH_TICK`; `IDLE_TICK` with no stream or nothing wanting more; a transport at rest
  parks `AT_REST_TICK`. A disconnected receiver reports ready forever: `changes` dropped and
  `Output::deaf` set the first time one does. `run` is two loops because a `Select` borrows its
  receivers and the stream's receiver lives in the `Output` every `&mut self` method replaces
  (`Heard` breaks the inner loop exactly when the set changed; `unsafe_code = "forbid"` ruled out a
  self-referential field).
- **Sinks are enumerated mid-stream on their own thread** (`Surveying`, a `resonate-sinks` thread
  holding the backend's `Surveyor`), so the engine never waits on the graph. One survey in flight; a
  change announced meanwhile leaves the flag set and the landing answer asks again. No thread:
  enumerate in line only while no stream is open, under `SINK_TIMEOUT`. **`Player::new` answers
  before the first survey:** the engine thread publishes the settings and answers, then surveys in
  line before it takes a command, so a window opens without waiting on the graph while the first
  `Load` still binds from a surveyed list (commands wait in the channel). A reader taking the list
  once waits on `Published::first_survey` through `Player::sinks_once_surveyed` (the binary's
  `confirm_sink`); the window polls the list and needs no wait.
- **A bind reads the published sink list; only an announcement refreshes it.** `select_sink` runs on
  every track change, non-in-place seek, settings change and renegotiation; enumerating there would
  put a `SINK_TIMEOUT` before each against a daemon that stopped answering. A list with no announced
  change since the survey last answered is current, empty included (an empty one is `NoSink` at
  once). A stale list still holding devices is bound from as published; the survey's answer moves the
  stream via `follow_the_sink_it_would_choose`. A list known wrong (stale or survey out, and empty or
  with a row waiting for a device or the graph) is not bound from: `wait_for_the_survey` keeps the row
  at its frame in `unbound`, publishes `Loading`, and the command answers `Ok`; the answer binds via
  `bind_the_row_waiting_for_a_device` or `bind_the_row_the_graph_let_go`, each of which reads an `Ok`
  that left no output as the row waiting again, never as bound.
- **Every stream opens off the engine thread.** `Backend::opener` hands out an `Opener` (PipeWire:
  the cloneable `Survey`, whose `open` waits up to `STREAM_OPENED_WITHIN`); `Streaming` is a
  `resonate-stream-open` worker taking one `Asked` at a time while the transport stays `Loading`.
  Each `Output` is numbered (`Output::made`): a stream landing for an output since retired or rebound
  is closed (`close_unwanted`). **One open in flight, none asked while one is** (a stray's `Close`
  must reach the loop before the next `Open`). `wait_for_the_graph` does not let go while an open is
  in flight or a row is opening, so an open refused `Disconnected` is read as the graph's, and a
  rebind leaves `graph_lost` standing until its stream lands (`waits_on_a_stream`: an output still
  priming asks its open later, so letting go there billed the open's refusal to the row). **An open the
  daemon never answers is the graph's, not the row's:** `Survey::open` past `STREAM_OPENED_WITHIN`
  answers `OpenUnanswered` (`Disconnected` once the client knows the daemon gone), which
  `parked_for_a_device` reads as the graph lost; read as `LoopStopped` it failed every queued row
  five seconds apiece. The ask and the loop settle one `Awaiting` by compare-and-swap, so a stream
  built after its asker gave up is closed on the loop rather than left playing as an orphan.
- **The engine decides which device the stream is on and follows the default itself.** Playback
  streams carry `node.dont-move`, so WirePlumber never relinks them (its `follow-default-target`
  moved the stream while `OutputStatus::sink` named the old device). Every sink-list refresh ends in
  `follow_the_sink_it_would_choose`: where `chosen_sink` (named device, else default, else first)
  answers a device other than the output's, the row is bound again at its position. A desktop's
  per-application device picker cannot move the stream; the settings pane chooses.
- **A command never leaves the transport playing nothing.** Rebind or start retires the old output
  before binding, so a failure for the row's own reason (`Convert`, `Decode`) would leave no stream
  while the transport plays: `carried_on_past_a_stranded_row` sends such an error through `fail`
  (reported, skipping on, engine `is_stranded`) and the command answers `Ok`. A device's error
  (`NoSink` to a `Load`) is answered as before.
- **A graph letting go of the ring is waited for once; the second time fails the track.**
  `RingProducer::is_abandoned` = consumer dropped (daemon restart). `watch_graph` closes the output,
  keeps the row with the heard position in `unbound`, publishes `Loading`; the sink survey runs and
  the landing answer binds the row again at that frame. Waits `GRAPH_BACK_WITHIN` (10 s) before
  failing the row; pause or stop ends the wait, another row does not: a bind failing with
  `Disconnected`, `LoopStopped` or a `Daemon` error while `graph_lost` stands is the graph's, not the
  row's (`parked_for_a_device`). The graph letting go again within that window raises
  `Error::LoopStopped` (`graph_last_lost`). A failing `backend.open` takes the whole `Output` with it
  (the consumer went into the call).
- **A device going is waited out, not billed to the row.** `Engine::fail` first hands its error to
  `parked_for_a_device`: `NoSink`, `SinkGone`, `StreamFailed` and `StreamRefused` (a backend's `StreamEvent::Refused`: the operation and the system's status, as RemoteIO's refused start) with a track open close the output,
  keep the row in `unbound` at the heard position, publish `Loading` (`Paused` where paused), mark
  the sink list stale, raise `Event::Waiting` not `Event::Failed`. Every sink-list answer then ends in
  `bind_the_row_waiting_for_a_device`, binding where the row was heard (on the fallback where the
  desktop has one, else the next device to appear). A stream failing again within `GRAPH_BACK_WITHIN`
  of the last loss is the row's after all (`device_last_lost`). With no output, `Engine::position`
  answers `unbound` where set, not the decoder's place, so `land_the_stream` reads the heard
  position into `unbound` before it drops an output whose stream would not open. `Engine::started`
  raises `Event::TrackStarted` and resets `renegotiations` whether or not the first bind found a
  device: a row parked for one is announced once, when it first plays nowhere else.
- **A fill is a slice, not a loop to a full ring:** `Engine::fill` decodes and writes for at most
  `FILLED_IN_ONE_GO` (20 ms), answering `Filled::ForNow` where it stopped with room left, so Pause or
  Stop is not held behind a deep ring's priming.
- **The PCM ring carries `u8`, not `f32`** (`f32` would convert every stream, breaking bit-accuracy
  for 32-bit sources). `ring_capacity` clamps the buffer setting between `MIN_RING_FRAMES` (or
  `BLOCKS_A_RING_HOLDS` of the widest block one pass writes, where more: the converting `fill` writes
  only while the ring has room for a block and `primed` waits for half the ring) and `LARGEST_RING`
  bytes, so a mistyped `buffer-ms` sizes a ring, not aborts. **Ring depth is the engine's, never the
  graph's:** every stream asks `LatencyRequest::Auto`, since a 100 ms-to-1 s ring written into a
  quantum would drag the whole graph to PipeWire's largest quantum.
- **A track change does not start the transport; a command does.** `Engine::start` opens the row the
  queue is on at whatever the transport was doing: `Next`, `Previous` and removing the playing row
  leave a pause in place. `Load { autoplay }`, `Play`, `Command::JumpTo` and an `Insert` asked to be
  heard set `playing`; a track failing mid-play still advances into playback. **Nothing to hear sets
  nothing:** `Load` with no rows leaves `playing` clear; `Play` over a queue with no current row
  answers `QueueEmpty`. **A toggle weighs what is heard, not what was wanted:** a load or `Next`
  answered `NoSink` leaves `playing` set over a track with no stream and nothing waiting for a
  device (`Engine::is_stranded`), so `TogglePlayPause` plays there, binding the row; a row waiting
  for a device is not stranded, the toggle pauses it.
- **A track change nothing will be heard through binds nothing.** Landing paused, `Engine::started`
  records the frame in `Engine::unbound` and stops: a skip run costs one open a row, not a sink
  selection, ring, DSP chain and `Backend::open`. Paused, every `rebind` records the frame instead;
  `Engine::play` spends it only where the bind made an output. `unbound` also holds the row of a lost
  graph, missing device or unanswered sink survey. `PlayerState::output` stays `None` until bound.

## The queue

- **Positions index the play order, not the load order:** `PlayerState::queue_position`,
  `Command::JumpTo`, `Command::Insert`, `Command::Remove`'s `Span` = rows a queue pane draws (right
  under shuffle). `Queue::position` = the heard row's load-order place (`None` while a queued row
  is heard), published as `PlayerState::loaded_position`; `QueueRows::loaded_at` gives every drawn
  row's place on the same count, so the two agree (the UI's `playing_entry` matches them).
- **`Command::Clear` removes every row but the playing one in one step** (every row where nothing
  is current): a clear made by position in two removals could land after the song had ended and
  take the row that had just begun, since advancing playback does not move the queue's revision.
  The phones' Clear is this command.
- **`Command::RemoveAll(Vec<Span>)` removes several runs in one step under one revision check**, the
  only way a gesture over rows that are not adjacent lands whole (a second `Remove` under the same
  revision would be refused `QueueChanged`); `Queue::remove_all` answers the first run outside the
  queue and removes nothing.
- **An edit by position names its queue.** `Player::send_if_the_queue_is_still` /
  `request_if_the_queue_is_still` carry the `Queued::revision` the gesture saw; `dispatch` answers
  `Error::QueueChanged` (`Cause::QueueMoved`) if it moved, so another client's edit between draw and
  click moves or removes nothing. `PlayerModel::send_by_position` sends the queue's `Remove`, `RemoveAll`,
  `Move`, `JumpTo`; `Insert`, the bus and MCP (rows named by id) are unguarded.
- **Ids.** A file no scan has seen takes its `TrackId` from `unclaimed_id` (down from `u64::MAX`).
  **A catalog file is queued under the catalog's id wherever a catalog exists** (`Host::held_as`;
  `AddTrack`, `OpenUri`, `mpris::claimed_by` and a resumed row ask before minting). **A row's id names
  that row alone:** `one_id_each` replaces an already-claimed id with a minted one in all
  `Queue::load` / `Queue::insert` input; the queue mints, not callers (a duplicate id made `Tracks`
  publish one object path twice). The ids held are the queue's own `Unclaimed` (`Queue::ids`), kept
  edit by edit (a removed or heard-next row releases its id; load and restore build it afresh), so an
  insert weighs only what arrives. `stamp_of` hashes location and span, not ids, so a renamed row
  still stamps as its playlist.
- **A queue edit costs what it moves.** A row is a `Held`: its `Arc<QueueItem>`, its *place*
  (the load-order count it arrived at: a load or restore numbers its rows from 0, every arrival takes
  the next, nothing renumbers) and its `RowStamp`. `order`, `unshuffled` and `next` are persistent
  `imbl::Vector<Held>`s, never indices into a row list, so a removal takes its rows out where the
  drawn positions say (a removed or heard-next row shifts nobody's index; a heard play-next row is
  `pop_front`); with shuffle off `unshuffled` is `order`'s clone, an O(1) share. **The publish shares
  the queue**: `Queue::published` splits `order` at `after` and appends `next` and the rest into a
  `QueueRows` (O(log n), every node shared with the queue and earlier publishes:
  `a_row_published_twice_is_the_same_row_and_not_a_copy`), and hands the list order over as the
  `unshuffled` and `next` vectors themselves (`Listing`), which `Queued::in_list_order` reads on the
  reader's thread. `QueueRows::in_load_order` ranks the places for `Keeping` there too. **The stamps
  are sums**: core's `QueueStamp` is the wrapping sum of a `RowStamp` (SipHash of the row and its
  place) a row, so a row arriving adds its term and one leaving takes its own away, and `Queued::stamp`
  and `playing_from` (`order`'s rows alone) never rehash the queue
  (`stamps_kept_edit_by_edit_agree_with_the_rows_stamped_afresh`). A place moves with nothing, so the
  rows a playlist loaded stamp as `stamp_of` of that playlist (place `i` for row `i`) while rows
  queued next come and go; a reorder leaves both stamps (only `Queue::split`, a move or reorder,
  sums `playing_from` again). A stamp is only ever compared, never stored. `Queue::relocate` copies
  only a row a reader still holds (`Arc::make_mut`) and keeps its stamp. **A drag splices**:
  `Queue::move_rows` cuts the span out of `order` and `next` where its drawn positions fall
  (`Queue::lift`, `Vector::slice`, O(log n)), reads which side of the rows played next it lands on
  from the row before its landing (`waits_beside`: a row heard or played next before it makes it
  play next, a span holding the heard row keeps every row's own side), splices the runs back at the
  landing and derives `after` from where the heard row stands; a row that changes side adds or takes
  away its stamp from `playing_from`, and with shuffle off `unshuffled` is `order`'s clone again.
  Under shuffle a drag inside the play order leaves `unshuffled` untouched.
  `a_drag_spliced_in_the_vectors_leaves_every_queue_the_rebuild_would` holds it to the rebuild
  (`flagged`, `split`, which `reorder` still is) for every span and landing over every seat and
  shuffle state. What stays linear: `reorder` rebuilds the three vectors from every row, and under
  shuffle an insert, a removal and a drag that moves rows between the play order and the rows played
  next walk `unshuffled` for where a row stands (the list order has no key a row's position could be
  found by: a place-to-position index needs order-maintenance labels or a counted tree, which
  `imbl` does not carry).
- **What was queued is apart from what plays.** `order` = loaded album or playlist (only list
  shuffle, a `RepeatMode::Queue` wrap and unshuffling touch); `next` = rows somebody asked to hear,
  in asking order; drawn = `order[..after] ++ next ++ order[after..]`. `next` is exhausted before
  `order` goes on; a heard or skipped-past `next` row leaves the queue. `PlayerState::queue_stamp` =
  `Queue::playing_from` (stamp of `order` alone, so `Library::playing_playlist` still badges the
  playlist while a queued row plays); `Queued::stamp` = every row, what `Keeping` weighs. Load
  replaces `order`, keeps `next`. `Command::Insert` lands as a `Placement`: `Next` (front of `next`),
  `Queued` (end of `next`), `At(row)` (joins `next` where the row before it is the one heard or a
  queued one, else `order`; nothing heard: all land in `order`). `play: true` goes through
  `Queue::insert_to_hear`, which jumps to the run's first row and, where the run landed in `next`,
  keeps the rest of it right behind (rows already queued play after the run), then starts it as
  `Engine::hear` (`Command::JumpTo`) does, so they cannot disagree about starting a row. `Queue::unshuffled` keeps the play order with shuffle off: a drag,
  sort or play-next while unshuffled edits it with `order`; not kept across runs.
- **Coming back is a command.** `Command::Resume` carries a `Resumption` (rows as playing, where
  each was loaded, row, frame, shuffled or not); sole caller of `Engine::start(at)` with other than
  `Frames::ZERO`. A frame past the row's end opens it at its start. It opens the row and stops
  (`unbound`): one open, one decoder seek, bind on first `Play`; not a seek (`Seeks` not stepped).
  Queue ids are not kept: `Queue::restore` claims a row's catalog id (`Resumable::held`), mints the
  rest. `Resumption::rows` = load order, `Resumption::order` = play order over it
  (`resonate-core::plays_in`: an order naming each row once stands, else load order); `restore`
  writes the shuffle flag, not `set_shuffle` (would reshuffle). Volume and repeat are deliberately
  not kept. Only bare `resonate` resumes; `resume` off discards what was kept. `Keeping` samples the
  `PlayerState` and published queue like a play and answers `Keep::Queue` (`Queued::stamp` moved),
  `Keep::Order` (`Queued::revision` or shuffle moved) or `Keep::Place` (row changed, position moved
  `KEPT_EVERY`, or transport rests elsewhere than the place kept); only the first allocates a
  `Resumption`. `resonate play` samples every 500 ms and keeps once more as it leaves
  (`Keeping::kept_on_leaving`: the same answers, but a place counts as moved at any distance).
- **Removing the playing row moves on as a skip** (`Queue::hand_on` = `advance` minus the
  repeat-track hold; else `Engine::remove` stops with `QueueFinished`).
- **A play is what was heard, sampled not hooked.** `Listening` takes a `PlayerState` beside the
  queue, sums frames the position advanced while playing (not for a sample whose
  `PlayerState::seeks` differs, nor a step over `A_SEEK` not counted as a seek), answers once per
  visit when half the track or `COUNTS_AS_HEARD` (whichever is shorter) has gone. A seek keeps the
  visit open except one back from within `A_SEEK` of a known end (repeat wrap), which begins another;
  a position going backwards with no seek counted restarts the count. A sampler because the write is
  SQLite: `RootView::count_a_play` and `resonate play`'s loop (`HEARD_SAMPLE`) run it off the engine
  thread. `Listening::heard`: `Counts(Played)` at the threshold, `Settles(Played)` once when a
  counted visit ends; a never-counted visit answers `Passes` as it ends if it reached
  `PASSES_AT_LEAST` (`Library::passed`, apart from plays: listening time counts it, a skip is no
  play). The pair joins by id, not location (`Library::track_played` answers the `Listen` it wrote,
  `Library::listened` spends it), so a failed write keeps nothing.
- **The sleep timer is the engine's (headless wants one).** `Command::SleepUntil(Option<Until>)`:
  `After`, `EndOfTrack`, `EndOfQueue`; `None` cancels. **Pauses** (a sleeper wakes where they were).
  End-of variants need only the edges in `skip`; the wrap would be missed, so `Queue::wraps_next`
  reads the two fields `advance` decides it from, and the fade treats a wrap as the end of the queue
  like the last row of a non-repeating one. Every `QueueFinished` (`Engine::finish_the_queue`: end of
  `skip`, `remove`, `fail`) disarms an end-of-queue timer, so the next load is not paused unasked. **Fades out over the last `SLEEP_FADES_OVER`**;
  cancel or push-back mid-fade restores the level, a seek or in-place restart lifts it. **A DoP
  stream is not faded toward sleep** (`Output::can_fade`): its ring cannot ramp and goes quiet at
  once without draining, so the position froze short of the end the timer waited for; it plays to
  the edge and pauses there. Outlives
  track change, seek, pause, new load. Due while a row opens, it pauses the opening. Delay capped at
  `LONGEST_SLEEP` so a huge value cannot overflow an `Instant`.
- **A skip under repeat-track repeats the queue, unless told otherwise.** `Command::Next` and
  `Command::Previous` going back a track are a person's skip (a track's end reaches `skip` as
  `natural`), so `skipped_by_hand` turns `RepeatMode::Track` into `RepeatMode::Queue`.
  `EngineConfig::skip_under_repeat` lives in the engine so media key, MPRIS `Next` and `resonate
  play`'s `n` obey it. A jump to a chosen row is no skip.
- **Previous restarts the song once past its opening, unless told otherwise.**
  `PreviousRestarts::starts_the_track_over` weighs the heard position (decoder less ring, chain and
  sink contents) against `PreviousRestarts::OPENING`. Past it under `RestartsTheTrack`,
  `Command::Previous` seeks to the start (a seek, steps `Seeks`); within it, on a track no longer
  than it, with no track open, or under `AlwaysGoesBack`, it retreats (`start`).
- **A relative seek past the end moves on, as the bus's `Seek`:** `Command::SeekBy` landing at or
  past a known length = `Command::Next`. Absolute `Command::Seek` past the end is still refused.
- **Published position never steps back within a stretch of listening.** Sink latency is known only
  once a stream reports it, so a rebind or seek would publish a position a latency behind, which
  `Listening` read as a new visit. `heard_position` holds at or above the last; released by a landed
  seek, a track started, a stop.
- **Giving up is counted per run of failures, not per track opened.** `Engine::fail` stops the
  transport once more rows have failed than the queue holds; reset by a track played out, a stop, or
  a person skipping or choosing a row, not by a stream opening (a daemon taking every stream then
  dropping it would loop a repeating queue for ever).
- **A row that will not open is passed over, not stood on.** A deleted file, unmounted disc or
  playlist row `--tidy` would drop answers `Error::Decode` from `Unwrapped::open`; `land_the_opening`
  hands it to `Engine::fail` (report, skip) where the transport was meant to play, else emits
  `Event::Failed` and sits `Stopped` on the row. `past_what_will_not_open` does the same for every
  command meaning play something. `Error::track` names the row in `Event::Failed`.

## What the engine publishes

- **An event is owed, not dropped, when nobody drains the channel.** `EVENT_SLOTS` fill while a front
  end is busy; the rest wait in `events_owed`, in order, so track changes cannot push out the
  `QueueFinished` headless `play` waits on. Past `EVENTS_OWED_AT_MOST` the oldest goes with a
  warning. `Underrun` is the one event let go, told at most every `UNDERRUNS_TOLD_EVERY` with missing
  frames summed.
- **One `Published` bundle, not a growing argument list:** `PlayerState`, `OutputSettings`,
  `StreamDigest`, queue, sink list, the visualiser's `Tapped`, each behind its own lock so a 60 Hz
  poll clones a pointer. The queue is a `Queued` (rows in play order as a `QueueRows` sharing the
  queue's own vectors, each row's load-order place, revision) under one lock; queue and digest are written before the `PlayerState` advertising them.
  `PlayerState::seeks` = token `Engine::seek` steps where the seek landed and nowhere else
  (`resonate-mpris` emits `Seeked` from it). `PlayerState::sleeping` carries what is left, so a front
  end keeps no clock. `OutputSettings` is the engine's whole configured output (a new `Arc` only when
  one moves), so the settings pane marks the chosen option from what is in force;
  `EngineConfig::sink` `None` = follow the default (`Setting::Sink` carries `Option<NodeName>`).
- **A command is answered once the state answering for it is published** (`Engine::answer` after
  `publish`, so `Outcome::wait` means the published state already says so). `Reply`: `Player::send`
  (unwaited; refusal announced as `Event::CommandFailed`), `Player::request` (refusal in its
  `Outcome`), `Player::settle` (a `Landing`; refusal still announced). `resonate-mpris` settles every
  engine-moving call via `Shared::settle`: zbus emits `PropertiesChanged` by calling the getter the
  moment a setter returns `Ok`, so a setter returning first announced the value just changed away
  from.
- **The inspector never rereads the playing file.** The engine samples `Decoder::last_packet`
  through a `ProfileBuilder` and publishes a `StreamDigest`; `resonate-ui` renders it without
  `resonate-codec`. The box layout is read by the open's own prescan (`Prescan::buffered`, whole
  sources only; `Decoder::layout`, `StreamReport::layout`), so a track opens its source once and one
  that is not a whole seekable source has no layout. A profile holds at most `MAX_WINDOWS` points
  whatever a packet claims to span. `Player::analyse` decodes the playing row whole on the window's
  background executor, only while the analysis pane is in front (`analysis.md`).
- **What is heard is tapped where written, read back by where the graph has got to.** `Tapping` sits
  on `Output` beside the ring and gets exactly the frames `RingProducer` took (after equaliser, gain,
  dither; before the ring's trim) as f32 left and right at the sink's rate, **stored at unity**:
  divided by the amplitude the chain rendered them at and read back multiplied by the level the ring
  hears now, so a mute draws silence and a turned volume shows at once. A `Tap` is a lock-free
  power-of-two ring of `AtomicU32` whose slots are allocated by the first frame recorded while
  somebody listens (`resonate play` and the bus never listen, so allocate none); the engine never
  waits on the window. The anchor (frames tapped less ring contents less `SinkStream::latency`) is
  fixed by `Engine::publish` every pass through a seqlock; `Tap::around` runs on from it by the clock
  and returns the frames centred on the one heard. **The window listens from the moment it opens**
  (`Player::listen_in`), not only while the visualiser is in front: a tap switched on as the pane
  opened held nothing for the ring's depth.
- **A queued row nothing plays is read once, off the audio path.** `Player::media` answers from a
  bounded catalog filled by `READERS` (four) `resonate-tags-<n>` threads running `probe`, so a source
  that never answers holds back only the reader it holds; a miss requests the read and answers
  nothing (`Player::media_revision` moves when one lands). It lets `resonate-ui` draw an unscanned
  file without `resonate-codec` and feeds `resonate-mpris` `GetTracksMetadata`. `Player::art` = the
  picture from the same entry, bounded apart (`ROWS_HELD`, `ART_BYTES_HELD`). A `Row` is a location
  and the span the queue row names; both live in one `Entry` under the location, so a cover is asked
  once however many rows a sheet cuts. Seeing a row unasked and claiming it is one operation under
  one lock (`Claim`), so the 60 Hz poll and bus thread cannot both enqueue a location; the guard
  never leaves `Shelf` (a `match` scrutinee holds its temporary for the whole match, so a `Catalog`
  locking the shelf itself would deadlock). What may be asked at once is bounded (`ROWS_ASKED`,
  `PICTURES_ASKED`, `LOOKS_ASKED`): a non-fitting ask answers `Sent::Backlogged` and the claim is
  released to `Unasked`, so the next poll asks for what is still on screen.

## DSP

- **Chain: remix, lossy restoration, resample, convolver, equaliser, gain, true-peak guard, dither**
  (equaliser: `eq.md`). The equaliser is after the resampler (a biquad's shape warps with its design
  rate) and before gain (volume is the last word, should attenuate a boost); the convolver is before
  it (both linear). An equaliser in force makes the plan `Converted`; a DoP-packed stream refuses it.
- **Resampler levels: one filter design at four lengths; High is default.** `Quality::params` is the
  whole difference (printed on hover in the settings pane, hence `SincParams` re-exported by
  `resonate-engine`). **Phase is its own setting** (`FilterPhase` `Linear`/`Intermediate`/`Minimum`,
  SoX `-L -I -M`): `phase.rs` folds the quality's linear kernel through the real cepstrum to minimum
  phase, designed once a run (`DESIGNED`, the design made outside its lock; a racing asker keeps the one held). **Reach is two numbers** (shaped kernels are asymmetric):
  `Reach { behind, ahead }`, history primed with `behind` zeros and flushed with `ahead` zeros; a
  track keeps its length whatever the phase.
- **Kernel polyphase, read position exactly rational:** `whole` sample index + `phase` numerator over
  phase count, integer-stepped (count = `SampleRate::ratio_to(output).numer`, 160 for 44.1 to 48 kHz;
  an f64 accumulator smeared those 160 into over a thousand). `Tabulated` builds all phases once in
  `Resampler::new`, each divided by its own sum (every phase passes DC at unity);
  `TABULATED_WEIGHT_BYTES_AT_MOST` is sized so every pair of common rates within 32:1 tabulates at
  every level, a rate no device offers uses `Kernel` (four-point Lagrange stencil). History is one f64
  plane per channel, `ACCUMULATORS` independent lanes, aligned to `VECTOR_BYTES` and zero-padded (no
  scalar tail); lanes pass through `lanes` before summing (LLVM's SLP vectoriser paired the two
  channels instead of packing each one's lanes).
- **Dither: flat or shaped path, f64; the plan's curve picks.** `Dither::prepare` resolves
  `NoiseShaping::at` the output rate once; `None` requantises with no error history and allocates
  none. Worked in target-grid steps in f64 (f32 rounded the noise near full scale). Output clamps to
  `[-1, 1 - step]`; fed-back error is taken before the clamp, and one not finite or past what the
  dither can make feeds back as nothing. A new `NoiseShaping` curve must say which path it takes.
  Lipshitz = E-weighted 44.1 kHz fit, resolved to flat except at 44.1 and 48 kHz; `Threshold` designs
  an order-12 error-feedback filter for the rate it runs at from Terhardt's (1979) threshold in quiet
  (minimum phase by construction) and is the `EngineConfig` default. `plan_for` stores the surviving
  curve in `OutputPlan::shaping`; `resonate explain` prints it.
- **Digital silence held a moment is handed on as digital silence:** dithered, a gap reached a 16-bit
  device as shaped hiss a DAC's silence detection never sees. Once every channel has read zero (or
  under `SILENCE_FLOOR`, where an equaliser tail rings) for `SILENT_FOR_BEFORE_MUTING_SECONDS`, the
  stage writes zeros and clears its error history. A fade is never silence.
- **`OutputMode` says what the plan does to the signal; a shorter word is not a resample.** Decided
  in `plan_for`, drawn as the chip on the playback bar and inspector: `BitPerfect` (stream = source's
  triple), `Repacked` (target holds every source value losslessly, container wider), `Dithered` (rate
  and channel map the source's, word shorter, dither covering it), `Converted` for all else (incl.
  narrowing with dither off). Stream request `no_convert` = `mode != Converted`. `dithers` resolves
  before the mode, so a narrowing with dither off (a truncation) carries no name saying it was
  covered.
- **Channel layout is negotiated like rate and format; one matrix is the whole downmix.**
  `plan_output` asks `SinkInfo::best_spec_for` for rate, format and layout together: the plan never
  names a triple the sink did not advertise
  (`every_spec_the_chooser_names_is_one_the_sink_advertised`). Fit is lexicographic: rate, channels,
  format. `Remix` runs first, so resampler and dither cost the sink's channels. Matrix from
  `ChannelPosition`: a channel the target has is copied at unity; lacking: folded into nearest
  neighbours at -3 dB (`MINUS_3_DB`); LFE dropped, not folded; an unfed target channel is silent.
  Routing is by position, never index, except where positions are unknown (`ChannelLayout::Discrete`
  either side): channel *i* to output *i* mod the output count at the one gain
  `1 / ceil(inputs / outputs)` (`in_turn`). Each row sums to at most one gain: no downmix clips a
  full-scale source.
- **The chain is f64 from decoded word to sink word.** `Processor::process`/`flush` take `f64`
  slices; the engine widens exactly into `Output::widened` and narrows once, into the sink's word, in
  `Output::stage` (an f32 carrier's 24-bit mantissa lost a 32-bit integer source's low byte).
  `build_chain` still names the carrier `SampleFormat::F32` (stages read only rate and channels).
- **Lossy restoration: spectral stage for MP3, AAC, Vorbis alone; off unless asked** (`Restoration`
  `Off`/`Repair`/`Extend`, `restore-lossy`; `LossySources` settings group has the `EXPERIMENTAL`
  badge). `Restore` is pushed after remix, before the resampler, at the source rate where the encoder
  cut. `Decoded` carries the source's `Tuning` (engine `tuning_of` names the lossy codecs, not
  `is_lossless`, which calls `Unknown` lossy too) and the lowpass wall, off `TrackHints`, else found
  as the music plays. Lossless sources never get it; one that does makes the plan `Converted`.
  **Repair** lifts the droop under the wall, fills holes with noise; **Extend** also rebuilds the
  wall to 97 % of Nyquist, never louder than 3 dB under its source.
- **A `Chain` carries a channel width per stage** (`Remix` changes frame width mid-chain). **One
  flush drains a chain; a flush with less room is refused:** `Chain::max_flush_frames` sums what
  every stage can still hand on and `Chain::flush` answers `Error::OutputTooSmall` for a shorter
  destination. One flush is the whole tail on purpose (a resampler pads half its filter, keeping a
  track exactly its length).
- **Clip prevention attenuates where the peak is known; only an unseen over is ridden down.** A
  ReplayGain boost is capped where the declared peak would reach full scale. `AppliedGain`
  (`resonate-core`) pairs gain with the selected peak and owns the arithmetic (DSP stage, `resonate
  info`, inspector cannot disagree). `AppliedGain::heeding` folds a measured true peak beside the
  declared one, keeping the larger (`true-peak` off leaves the declared peak). The per-sample clamp
  survives only where a boost has no peak and no guard follows; with `true-peak` on, `gain_of` sets
  `GainConfig::guarded_after` and overs reach the guard whole. A boost capped at exactly unity
  changes no sample: `AppliedGain::adjusts` false, the track plays bit-perfect.
- **What the catalog studied reaches the player through `Hinting`, beside `StandIn`, not gated on the
  vault.** `resonate-core::TrackHints` (true peak, lowpass wall, measured gain) is core vocabulary;
  `Library::hinting` answers from `track_studies` by the row's path and span; `Unwrapped::open` asks
  once and folds it into the gain in `levelled`. **A track nobody studied is measured where its peak
  would matter** (`true-peak` on, non-DoP sample-packed source, gain in force has no peak and asks
  for a boost or source is float): `measure::Measuring` decodes the row whole on a `resonate-peak`
  thread through a `TruePeakMeter`. `Track::peak` is a `Peak` (`Unasked`, `Measuring`,
  `Unmeasurable`) so a row that will not decode to its end is not decoded again per settings change.
- **Samples past full scale are counted where nothing guards them.** With `true-peak` off a plan that
  runs the chain ends it with `Overs` (`resonate-dsp`), a pass-through stage after gain counting
  samples above unity beside the dither that would clamp them; `GainStage` counts its own per-sample
  clamp. `Processor::overs` is the running count, `Chain::overs` their sum, banked per chain in
  `Output::overs_of_retired_chains` so a retune keeps it, and `OutputStatus::overs` is the
  total for the open stream (the inspector's *clamped samples*; a first sample warns once per
  stream). It is plain integer arithmetic on the engine thread: no atomics, no allocation
  (`a_boost_that_reaches_the_clamp_with_true_peak_off_is_counted_and_a_cut_is_not`).
- **True-peak guard: a lookahead gain, not a clipper; touches nothing under the ceiling.** `TruePeak`
  (`resonate-dsp`) is pushed after gain, before dither, wherever `true-peak` is on and the plan
  converts, never on bit-perfect or repacked streams. It reads each frame oversampled by rate (`Sampling::at`: 4x under 96 kHz, 2x under 176.4 kHz,
  none above, where a read is the centre tap alone; about 192 kHz of grid in every case, so the guard's
  worst error on a tone near Nyquist is BS.1770's 0.7 dB, not the meter's tenth); where a phase
  passes -0.1 dBTP it asks for the gain bringing it under, averaged over the lookahead. A stream
  never passing the ceiling is multiplied by exactly one and comes out bit for bit, only later:
  audio waits `lookahead + 48` (`REACH`) frames, handed on in its flush; `Engine::swap_chain` first
  flushes the running chain's held tail into the ring. The interpolator is 96 taps (every fractional
  phase must pass a tone near Nyquist at its own level inside the 0.1 dB between ceiling and full
  scale; the study's `TruePeakMeter` keeps the 8x grid, as it knows no rate), skipped where nothing can be over
  (`quiet_below`).
- **Pre-amp and an untagged track's gain are part of what ReplayGain asks for; clip prevention weighs
  them.** `Levelling` = a `Trim` each (quantised millibels, so `OutputSettings` keeps `Eq`);
  `resolve_replay_gain` folds them into `AppliedGain`: tagged gain raised by pre-amp keeps its tagged
  peak; no gain declared takes `untagged` with no peak (the one case the per-sample limiter is for).
  **A studied track declaring no gain is levelled by what was measured** (`TrackHints::measured`, a
  `MeasuredGain`: -18 LUFS less integrated loudness; album: duration-weighted energy mean, only where
  every album track is studied), used by `engine::tagged_or_measured` only where tags carry neither
  gain (the vault strips a delivered row's tags).
- **The plan alone judges a gain stage; a converting plan always carries one.** `gain_config`
  answers `None` at full volume under unity gain, but `plan_for` asks for a stage anyway wherever the
  plan resamples, remixes, equalises, convolves or restores (a stage already there makes every
  volume/ReplayGain change a retune, not a shape change). `dop_survives` still reads `gain_config`.
  `Engine::fill` and `Engine::flush` pick byte copy or conversion from `OutputPlan::is_transparent`.
- **Every float reaching an integer word is rounded to nearest, ties to even, saturated (NaN to
  zero); one set of core conversions is the whole of how** (`quantise::f32`; `SampleData::write_f64`
  is what `Output::stage` narrows through to the ring, `write_f32` its narrow twin). A narrowing
  nothing dithers is `OutputPlan::rounds`: non-transparent plan, empty chain, so samples reach
  `write_f64` rather than symphonia's flooring shift or a truncating cast (dead band a step wide
  around zero). Rounding adds a constant making IEEE round the sum to a whole, half to even, instead
  of `round_ties_even` (the `x86-64` baseline has no `roundps`; the addition vectorises on SSE2).

## The sink

- **A sink's formats are re-read when the node says they moved.** A port switch or EDID re-read can
  change a node's formats while the node stays; its `info` event is watched for `PARAMS`, the
  `EnumFormat` entry's `SERIAL` flag flipping per change (`SinkRecord::formats_moved`);
  `SinkChange::Reformatted` has the engine survey afresh. **A running device says its own rate:**
  `SinkInfo::current_rate` = the sink node's `Format` param rate (present only while running; same
  `SERIAL` watch), else `clock.force-rate`, else `clock.rate`.
- **A global leaving the registry takes its proxy with it, whatever it was told first** (a card
  unplugged before its routes were enumerated keeps no proxy/listener until reconnect). **A stream's
  listener goes before its stream:** `pw_stream_destroy` frees the hook list the listener's
  `spa_hook_remove` writes into, so an open stream is an `Opened` whose `Drop` takes the listener
  first, never a tuple.
- **The client outlives its daemon.** A connection's everything (core, registry, listeners, bound
  proxies) is one `Graph`, made by `Reaching`; the loop keeps main loop and context for the process's
  life. A core `error` event with broken pipe, reset, abort or missing connection = daemon gone: it
  sends `Request::Lost` through the loop's own channel (a connection cannot be torn down in its own
  callback); `Lost` drops streams and graph, answers every pending `Sync` by dropping it, empties
  `Discovered`, announces each known sink removed; a thread sends `Request::Reconnect` a second later
  until a core connects, **the first connect being one more try**. **A daemon silent without closing
  is lost too:** `Request::Heartbeat` every `HEARTBEAT_EVERY`; a beat unanswered after
  `DAEMON_ANSWERS_WITHIN` goes through `lose_the_graph`. **Nothing inside the request callback sends
  on the request channel** (pipewire-rs runs it under the channel's lock; a send from there never
  returns). With no graph, a `Sync`, open or capture answers `Error::Disconnected` at once. **A
  recording survives the restart:** `resonate-listen`'s `Capture` reads a disconnected events
  channel, or a `StateChanged` to `Failed` on an open one (an unplugged microphone), as lost and asks
  the same client for the same capture every `LOOKS_EVERY`. **A gap is silence of the time lost, not
  a splice:** the reopened stream's `Recorder` is held back until `Recording::silence_for` has
  advanced the clip by the time since the loss in whole frames (the slots are zero until written),
  so the clip keeps wall-clock timing and the deadline is not extended; a reopen that fails adds no
  silence, so a microphone gone for the whole clip is still `NothingHeard`. **A stop that fails keeps
  the clip heard** (`finished`: warned; the error stands only when nothing was heard).
- **A metadata key cleared reads as unset.** `Discovered::heard` reads `settings` and `default`
  metadata keyed by speaker (`HeldIn`): a key with no value clears what it held; a key of nothing
  clears every key that object carries and no other's (as `pw-metadata -d`). **Only the core
  subject's keys are read** (subject 0, `CORE_ID`): `default` metadata also carries per-node keys
  (WirePlumber's `target.object`), cleared when the node leaves; reading that as every key cleared
  wiped the default sink. A metadata object leaving the registry is the same clear.
- **The chosen sink is named, not numbered:** `EngineConfig::sink` and `Command::SetSink` carry a
  `NodeName`, matched by `select_sink` on every stream open (PipeWire ids are per object; a replugged
  device gets a new one). `OutputStatus::sink` stays a `SinkId`. An unanswered name warns and falls
  back to the default until the device turns up.
- **Hardware-ness is a question about the `Device` above the node.** A `Node` global carries
  `device.id`, not `device.api`; `Discovered::is_hardware` answers for the pair at snapshot time, so
  the globals may arrive in either order. **The physical port is the `Device`'s to answer; the seat
  is the join:** a card's `Route` params name its ports; a sink node's `card.profile.device` names the
  seat on the card (`DevicePorts::serving`), read off the node's `info` only where the change mask
  says `PROPS` (a volume change raises `info` with no props, which must not read as no seat).
  `Route` = the port the card is switched to, outranks the rest; `EnumRoute` = every port offered,
  the only place an unplugged port appears. `Plugged`: `Yes`, `No`, `Unsaid` (no jack detection).
  `HardwareVolume` from `route.hw-volume`: `Yes`, `No`, `Unsaid`, never drawn as `No`.
- **A device turning its own volume can take the slider; the stream then stays bit-perfect at any
  volume.** `device-volume` (off by default) = `EngineConfig::device_volume`. `Attenuator::of` weighs
  it against `SinkInfo::turns_its_own_volume` (route says `route.hw-volume` is `Yes`, never
  `Unsaid`); `Attenuator::leaves` = the volume the stream still applies (`Volume::MAX` where the
  device takes it; a ReplayGain adjustment stays the stream's). `PipeWire::set_device_volume` sets
  `Route` on the `Device` (what pipewire-pulse writes for a desktop slider; node `Props` would be the
  adapter's software volume), **keeping balance and mute**. **The slider starts where the device is**
  (`Volume::heard_at`) and follows a device turned from the desktop: a `Route` is not re-sent when
  volume moves, so the device's `info` saying `PARAMS` changed enumerates again (`SinkChange::Turned`,
  `follow_the_devices_volume`). The engine's own sends echo back the same way: `Turns` (`turns.rs`)
  remembers the last `TURNS_REMEMBERED` (8) gains and every one sent within `ECHOES_ARRIVE_WITHIN`
  (2 s, at most `TURNS_HELD_AT_MOST`), so a fast drag's early echoes still arrive known; a reading
  within `A_DEVICES_HALF_STEP_DB` (0.75 dB, half a coarse hardware step) of any is an echo, since a
  device rounds what it is sent to its own steps (silence matches silence alone). Read as turned
  elsewhere, an echo snapped the slider and cleared the mute. Desktop mute is kept apart from level (`OutputStatus::device_muted`).
- **Stream latency is counted in the stream's frames, converted where the graph tick rate is
  known.** `pw_time.delay` is in `pw_time.rate` (graph clock), not the stream's. The `process`
  callback holds the only consistent delay/rate snapshot: it builds a `GraphTime` and publishes
  `downstream(spec.rate)` as one `AtomicU64` of stream frames (two atomics would tear; converting
  later takes a lock the RT thread may not). `GraphTime::buffered` and `GraphTime::queued` are
  already at stream rate (`pw_time.queued` stays zero because pipewire-rs sets no `pw_buffer.size`).
- **Bit-perfect is what the graph runs at, read every cycle, not what the stream opened at:** another
  client can hold the graph at a second rate after the stream opened. `StreamClock` carries
  `GraphTime::graph_rate` from the callback to `OutputStatus::graph_rate`; `OutputStatus::hears`
  demotes the published `mode` to `Converted` wherever `converted_by_the_graph`. **So is the
  stream's own volume:** the session manager restores a saved per-stream volume and a desktop mixer
  turns or mutes the stream, both as `Props` on the stream's node, and the engine's own volume never
  goes there (it is the chain's, the ring's or the device's). `param_changed` reads every `Props`
  into `StreamLevels` (`props.rs`; a pod naming one key keeps the others it named before) and sends
  `StreamEvent::VolumeChanged(StreamVolume)` when the verdict moves; `scaled_by_the_stream` demotes
  the mode the same way, without reopening. The plan is untouched: `no_convert` and every reshape
  still read `OutputPlan::mode`.
- **The callback fills the quantum the graph asked for, not the buffer handed over:**
  `pw_buffer.requested` = quantum in frames (`Cycle::asked_for`), clamped to the pool's room, falling
  back to all of it where the graph names nothing. Filling the whole pool buffer drained the ring
  many quanta at a time and declared an underrun against that much want.
- **Silence about a capability is not a refusal.** A node answering `EnumFormat` with no channels
  property, or no formats, is believed about what it said and left alone about the rest: candidates
  fall back to the source's own layout, or `allowed_rates` crossed with the source's format.
  `SinkInfo::supports` stays strict (exact format at exact rate) but a format entry naming no
  channels takes any layout (`SinkFormats::takes`). A sink's advertised formats and the graph's
  `allowed_rates` are separate fields: a device advertising 192 kHz is irrelevant if the daemon will
  not switch the graph to it.
- **Twenty-four bits: one depth, two words; the wire word is the graph boundary's alone.**
  `SampleFormat::S24` = sign-extended low 24 bits of an `i32`, four bytes everywhere in the
  workspace; a device may want three. `format::WireWord` (`resonate-pipewire`) is the difference:
  `sample_format` reads `S24LE` and `S24_32LE` as one depth, `spa_format` takes the word as a second
  argument. `build_stream` offers both, packed first (a conversion avoided is the bit-perfect
  point); the graph's choice returns via `param_changed` as `StreamEvent::FormatChanged` and is kept
  on `OutputStatus::words`. **Packing is in the graph's own buffer, forward, allocation-free:**
  `process::pack` reads the low three bytes of the word at `4i`, writes them at `3i`, never reaching
  an unread word; the ring still fills at the padded stride and `Silence::write` still lays DoP
  markers as four-byte words (the marker is byte 2, so a DoP carrier survives).
- **The settled layout is read from the format's positions, never its count** (`format::placed`):
  the `position` array is matched against `spa_position` of each named layout, else `Discrete`; a
  format naming no positions keeps the layout asked for where the count agrees, else `from_count`.
  So a 5.0 or 6.1 stream reads back as itself and is not rebuilt
  (`every_layout_asked_for_reads_back_as_itself_once_the_graph_settles_on_it`,
  `a_graph_settling_on_the_five_point_oh_it_was_asked_for_leaves_the_stream_alone`); one that truly
  differs plays silence until the rebuild (`realtime.md`). A hosted daemon cannot prove this: with
  no session manager nothing links, so no `Format` is ever set.
- **A sink's advertised layouts are read the same way** (`format::advertised_layout` over
  `named_by`, the one matcher): `parse_enum_format` collects the `channels` offer and the `position`
  array whatever their order, and gives each offered count whose array is that long and names a
  channel the layout holding the same set of places (`holds_the_places_of`), else `Discrete`; no
  array, an all-zero one or one of another length falls back to `from_count`. The set, not the
  order, decides, since the stream names its own positions and the graph routes by name: ALSA's
  5.1 (`FL FR RL RR FC LFE`) is 5.1. A side pair stands for a rear pair the device lacks, as
  PipeWire's own 5.1 places its surrounds on `SL`, `SR`. So a 5.0 or 6.1 device lists as itself (a
  matching source is not remixed by index) and a 6.0 one is not listed as 5.1
  (`every_named_layout_a_sink_places_is_advertised_as_itself`,
  `a_five_point_one_sink_is_itself_in_alsa_order_or_with_its_rears_on_the_sides`,
  `an_advertisement_through_the_real_parser_keeps_its_positions`). The settled format stays matched
  in order by `named_by`: it echoes the positions the stream asked for.

## Fixtures

- **Capture and playback tests on the desktop graph skip with no daemon:** `PipeWire::start` starts a
  client before a daemon exists, so `resonate-listen`'s `tests/capture.rs` and `resonate-pipewire`'s
  `tests/stream.rs` read `Error::Disconnected` from the first sink discovery as a skip. Other
  discovery failures fail; reconnect tests host their own daemon. A hosted daemon proves what the
  client reads and asks, not what flows: with no session manager nothing is linked, and WirePlumber is
  not hosted because it would reach the real cards. A hosted test playing the other client turns a
  stream by its node's id (`the_playback_node`, read off `pw-cli ls Node`): the application name
  is the client's as well as the node's, and `pw-cli` resolves a name to the client, which refuses
  `set-param` yet exits zero.
- **A test asserting which row plays pauses the transport first** (`stand_still`): a decode failing
  on a loaded machine reaches `Engine::fail`, which skips.
- **Real-file fixtures are built by ffmpeg at test time; the suite skips without it.**
  `crates/resonate-codec/tests/encoded.rs` also builds a rip through `flac`/`metaflac`, MP3s through
  ffmpeg's `libmp3lame`, and writes one CAF itself (ffmpeg's muxer refuses AAC).
  `RESONATE_REAL_FIXTURES` points the suite at a folder of real files: every file must probe, name a
  known container and codec, report a duration and decode its first block. Unset: skip.
- **A bus test reaches its own service by the connection's unique name** (`Mpris::unique_name`),
  never the well-known name: every harness in the process claims `org.mpris.MediaPlayer2.resonate`
  with replacement. **It waits for a sounding track, not the status** (`Harness::sounding`): a row
  still opening is published `Buffering`, read `Playing` on the bus with no current track, and a
  `SetPosition`, `Next` or track id read then is rightly ignored.

## Platform boundary

Sink identities, formats, timing, stream control and backend traits live in `resonate-audio`;
it reaches core, not a device API. PipeWire implements the traits in its own crate and reexports
the moved vocabulary for existing callers. The engine has a PipeWire dependency only outside
Android; Android constructs it with `Player::with_sources_and_backend`.

A source registry may supply a writable spool folder with `Sources::spooling_under`. Only that
folder is tried when supplied; the Linux default remains `TMPDIR`, else `/var/tmp` then the
system temporary folder. This lets a sandboxed document stream spill without changing process
environment variables or treating a document URI as a path.
