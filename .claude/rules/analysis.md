---
paths:
  - "crates/resonate-analysis/**/*.rs"
  - "crates/resonate-library/src/studies.rs"
  - "crates/resonate-library/src/fingerprint.rs"
  - "crates/resonate-ui/src/analysis.rs"
  - "crates/resonate-ui/src/analysis_plot.rs"
  - "crates/resonate-ui/src/views/analysis.rs"
  - "crates/resonate-online/src/acoustid.rs"
  - "crates/resonate/src/analyse.rs"
  - "crates/resonate/src/studies.rs"
---

# Analysis, studies and recognition

`resonate-analysis` decodes a track whole and says what it is: how it moves over time, what its
spectrum holds, whether its container's lossless claim holds, how loud it is and what its audio
prints as. A leaf beside the vault — `resonate-core`, `resonate-codec`, `resonate-dsp` (for the
true-peak meter the engine's guard shares and the resampler), `rustfft`, `rusty-chromaprint` and
`smallvec` — and `cargo tree -p resonate-analysis` stays free of gpui, the engine, the library and
`ureq`. The engine re-exports its vocabulary and answers `Player::analyse` through the player's own
`Sources`, so the window reaches it through the stand-in and a vaulted row is analysed out of its
object; the library reaches `study` for the enrichment's studies.

## One pass, many builders

- **`analyse` and `study` are one walk, differing in a type.** `walked` opens the decoder
  (`open_span` for a cut) at the width the vault's `pcm_of` would choose, so integer samples keep
  their low bits, and hands every block to the builders. `Drawing` is the seam: `Unseen` for a
  `study`, `Drawn` — envelope and spectrogram — for an `analyse`, so a library-wide study builds
  neither thing only a screen wants. DSD is decoded to F32 PCM through its decimator, not as DoP.
- **A sample that is not a number is weighed as silence.** `normalise` hands every builder the
  block as `f32`, and a float file's NaN or infinity becomes 0 there, as the engine's equaliser
  guards its own state — one such sample once rode the K-weighting's history to the end of the
  track, leaving loudness and range `None` and the spectrum's sums non-finite
  (`a_sample_that_is_not_a_number_is_weighed_as_silence`). The bit reading keeps the native
  samples, where one only reads as off every grid.
- **`Watching` is how a caller stops and follows a pass.** `Watch` is the window's — atomics for
  frames done and expected and a stop flag — and `EnrichProgress` implements the same trait, so
  cancelling a lookup stops every study at its next block. A stopped pass is `Error::Stopped`,
  which every caller reads as no answer rather than a failure.
- **What would grow without bound is held in `Doubling`.** A column type that `Absorbs` is pushed a
  unit at a time, and whenever the columns reach the cap every pair merges and each holds twice
  the units, so a stream of unknown length ends in at most `ENVELOPE_COLUMNS` or
  `SPECTROGRAM_COLUMNS` columns, all whole but the last. `Envelope::condensed` reaches any width,
  reading each output column off the frames it spans.
- **The spectrum is a Hann-windowed transform of every channel but the LFE, sized by the rate**:
  4 096 points at 44.1 and 48 kHz, doubling per doubling of the rate, so a bin is ~11 Hz anywhere;
  each bin the mean of the channels' powers, scaled so a full-scale sine on a bin in every channel
  reads 0 dB. Not a mono mix: a mix cancels a stereo pair near −1 correlation to near silence,
  which left it `NotJudged`, and sums the LFE's rumble in with the speakers
  (`a_stereo_file_in_opposite_phase_is_judged_by_what_its_channels_hold`,
  `the_low_frequency_channel_is_left_out_of_the_spectrum`). Two channels go through one complex
  transform, one as its real half and one as its imaginary, each bin's pair of powers read off the
  bin and its mirror, so a stereo file costs the single transform a mix did and a 5.1 one three.
  The long-term average leaves out every window whose mean square over those channels is quieter
  than `SILENT_BELOW_DB`, since fades and gaps would pull it towards the floor it is weighed
  against; `Spectrum::heard` is how much was loud enough to count. The spectrogram keeps each row's loudest
  bin on a linear axis from nothing to Nyquist, where a lowpass wall is a horizontal line (a curve
  on a logarithmic one). `Spectrogram::painted` writes it as a `resonate_codec::Raster` of BGRA
  pixels through a `Ramp` the caller passes, so the theme stays in the window.
- **The bits in use are the trailing zeros of every sample ORed together.** A 16-bit master in a
  24-bit container leaves the low eight bits zero; a float file on the 16-bit grid is the same in
  another shape. Weighed beside the declared depth, never in place of it.
- **Loudness is BS.1770, the DR reading the TT meter's.** The K-weighting's two biquads are derived
  for the stream's own rate
  (`the_k_weighting_at_48_khz_is_the_one_the_recommendation_tabulates` holds the derivation to the
  tabulated coefficients), over 400 ms blocks at 100 ms steps, gated at −70 LUFS and 10 LU under the
  ungated mean. A channel is weighed by where its speaker sits: `resonate_codec::Speakers::placements`
  reads the source's positions (the WAVE order where it names none); the LFE weighs nothing, a side
  speaker 1.41, a rear one 1.41 only where no side pair stands before it, all else 1.0 — the
  recommendation's ±60°–±120° rule read off names, not angles. The loudness range is EBU Tech
  3342's: 3 s short-term blocks at 100 ms steps, gated at −70 LUFS and 20 LU under their mean, the
  spread from the 10th to 95th percentile of what is left; the momentary and short-term maxima are
  the loudest 400 ms and 3 s blocks. All four read what `ffmpeg -af ebur128` reads, to the tenth,
  on a stereo master and on a 5.1 mix whose LFE carries a whole channel. DR is the second-loudest
  3 s block's peak over the RMS of the loudest fifth, averaged over channels and rounded. The true
  peak is `resonate-dsp`'s `TruePeakMeter` over the same blocks: every channel at eight times its
  rate through a 96-tap windowed-sinc interpolator, all eight phases in one pass, the loudest kept —
  the reading the engine's guard is built on, so study and playback cannot disagree about an over.
  A study taken under the earlier 48-tap interpolator kept its stamp: `JUDGED_UNDER` names the
  verdict, and what it read low is only content above 90 % of Nyquist, which the guard reads again
  live wherever the plan converts. It is `Loudness::true_peak` and `track_studies.true_peak`, what
  `Library::hinting` hands the player beside the ReplayGain the integrated loudness asks for, which
  levels a track whose tags declare none (`audio.md` has the rule).
- **The print is Chromaprint's own algorithm, first two minutes only.** `rusty-chromaprint` under
  `Configuration::preset_test2` (the preset libchromaprint and AcoustID default to), fed 16-bit
  samples for `PRINTED_FOR`, compressed and written in URL-safe base64 without padding, which is
  what AcoustID reads. `resonate_core::Chromaprint` carries it with the whole track's length, the
  duration a lookup is asked with. `print` stops decoding once the print is full only where the
  container declares a length; one that declares none is decoded on to its end, printing nothing
  more, so the length asked with is the stream's rather than the two minutes printed
  (`a_stream_that_declares_no_length_is_printed_as_long_as_it_plays`). A stream the printer refuses
  to start is a debug record and `print: None`, never a failed analysis.
- **A clip is signed, never printed.** AcoustID's lookup filters by the `duration` it is sent,
  read as the whole recording's, and matches prints taken from a recording's start; a clip Listen
  heard has neither — its own twelve seconds sent as the length matched only recordings twelve
  seconds long — so no clip is sent to AcoustID. Listen asks the services that match a snippet,
  Shazam and AudD, and AcoustID is asked only about a file printed whole, by the lookup and the
  Analysis pane. `signature_of` is Shazam's signature, written from the format as documented rather than any
  client's source: the clip mixed to mono, resampled to 16 kHz through `resonate-dsp`'s `Resampler`
  at `Balanced`, the middle twelve seconds kept and scaled to the 16-bit range; a 2 048-point Hann
  transform every 128 samples, its power over 2¹⁷ held in a ring of 256 frames beside a spread
  taking each bin's maximum over it and the next two and folding into the frames one, three and
  six back; a peak read 46 frames behind the newest where the power reaches 1/64, the spread just
  below it, eight neighbouring bins of the spread 49 frames back and fourteen other frames of it,
  its magnitude `max(ln p, 1/64)·1477.3 + 6144` and its bin refined to a 64th by the parabola
  through its neighbours. Peaks are filed into 250–520, 520–1 450, 1 450–3 500 and 3 500–5 500 Hz
  and written as the service reads them: a 48-byte header of `0xcafe2580`, a CRC-32 of everything
  past the eighth byte, the size, the 16 kHz rate id shifted by 27 and the sample count plus a
  quarter-second's worth; then each band's tag, length and peaks — a frame delta, escaped with
  `0xff` and the whole frame where it will not fit a byte, then magnitude and bin — padded to four
  bytes, handed on as a `data:audio/vnd.shazam.sig;base64,` URI. A steady tone signs as nothing, a
  peak having to stand out in time as well as frequency;
  `a_tone_leaves_its_peaks_in_the_band_it_sounds_in` plays notes that start and stop.

## What the window drew is kept

**An analysis is kept against the file it was taken of, and a changed file forgets it.**
`KeptAnalyses` is a folder of one file per analysis — `$XDG_CACHE_HOME/resonate/analyses`, handed
to the window's `Player` through `Player::keeping_analyses` — named by an FNV-1a of path, span,
size and modification time, so a rewritten file or a cut taken again is a name nothing answers to.
`Player::analyse` asks it before decoding and keeps what it decoded, so the pane opens on a track
drawn before a restart without reading the file. `kept::written`'s layout is a magic, a version,
the `JUDGED_UNDER` it was taken under, every field of the study, the envelope and the spectrogram,
little-endian. A file stamped under another `JUDGED_UNDER` reads as nothing, as the catalog studies
a track again whole on a bump rather than judging it again, so the pane never shows a verdict of
this build beside a loudness, true peak or print of an older one
(`an_analysis_taken_under_another_heuristic_is_not_read_back`). The judgement is *not* written —
`read` runs `judged` over the kept spectrum and levels. The reader trusts none of its own shape: an envelope naming no lane or
more than `ENVELOPE_LANES`, or columns of no frames over frames, reads as nothing, and
`Envelope::condensed` answers nothing for either however it was built — an index past the lanes
and a division by zero once reached the pane
(`a_kept_envelope_whose_shape_could_not_have_been_drawn_is_not_read_back`). A file that will not
read back is deleted where it stands, a write
lands through a staged name and rename, a recall touches the modification time, and past
`KEPT_BYTES_AT_MOST` (256 MiB, ~four hundred tracks) the least lately used go first. A cache:
every failure is a debug record and a decode. The headless commands keep nothing.
`an_analysis_reads_back_as_it_was_written_and_a_truncated_one_does_not`,
`an_analysis_is_kept_against_the_file_as_it_stands_and_forgotten_once_it_changes` and
`what_is_kept_is_trimmed_to_its_bound_the_least_lately_used_first` are the claims.

## The verdict

`verdict.rs` is pure: a `Weighed` of codec, rate, declared depth, spectrum and levels in, a
`Judgement` out — a `Verdict` (`Genuine`, `Suspect`, `Fake`, `Lossy`, `NotJudged`) with typed
`Finding`s. `JUDGED_UNDER` names the heuristic and is bumped by hand whenever its answer, or a
measure a study keeps, could change, since a stored study and a kept analysis are weighed against
it.

- **A wall is a steep drop to a floor that stays there.** The spectrum is read in 100 Hz bands up
  to `TOP_GUARD` of Nyquist. An edge is a wall where the mean of the bands from a kilohertz to
  200 Hz below it stands at least `WALL_DB` over the mean of every band from 200 Hz above it to the
  top, *and* at least `WALL_DB` less `WALL_FLOOR_SLACK_DB` over the loudest of them — the second
  keeping a gentle slope from reading as a wall, a slope's loudest band above the edge being the one
  just past it. The highest run of walled edges is taken and its steepest is the cutoff, so a wall
  is found where content ends, not where it first thins.
- **A wall is looked for twice: in the average and in what a quarter of the windows stay under.**
  A loud master decoded from a lossy encode clips where its overs came back over full scale, and a
  clipped run's splatter fills the average above the encoder's lowpass — a clipped 128 kbps AAC's
  average *fades out by 21.9 kHz* while the wall was still in every unclipped window. So
  `Transforming` keeps, beside the running sum, a histogram of every loud window's level per 100 Hz
  band — a decibel a bucket from the floor to full scale — and `Spectrum::typical` is each band's
  `TYPICAL_QUANTILE`, a quarter, a level only a window's worth of transient splatter rises above.
  `judged` runs `wall_in` over both and `lower_of` takes the lower wall where both find one, the
  one there is otherwise, so a burst no longer hides a cutoff and a steady wall reads where it
  always did. The histogram is `BUCKETS` (161) `u32` counters a band — ~140 kB at 44.1 kHz,
  ~620 kB at 192 kHz.
  `a_transcode_splattered_above_its_wall_by_clipping_is_still_fake` is the claim.
- **Where the wall stands is what it means.** At or under `LOSSY_CEILING_HZ` (19.5 kHz) it is where
  a lossy encoder cuts: `Fake`, with a `LossyGuess` read off LAME's lowpass table; up to
  `SUSPECT_CEILING_HZ` (20.7 kHz) `Suspect`, the band where a high-bitrate encode and a genuinely
  low anti-alias filter cannot be told apart; above, an anti-alias filter. A stream above 48 kHz is
  weighed against the rates it could have been upsampled from — only those at most half its own,
  so a 96 kHz file is never called an 88.2 kHz one upsampled — and a wall no higher than one of
  their Nyquists plus `UPSAMPLE_SLACK_HZ` is `Upsampled`, which is `Fake`.
- **No wall is not a pass.** A spectrum reaching past `SUSPECT_CEILING_HZ` — or, hi-res, past where
  the highest rate it could have been upsampled from would end — within `CONTENT_WITHIN_DB` of its
  1–4 kHz level is `Genuine`; one fading below that with no wall is `NotJudged`, since an old
  recording and a band-limited master fade as a transcode does not.
- **What the spectrum cannot judge is not judged.** A lossy codec is `Lossy` whatever its spectrum,
  still reporting the wall with a guess; DSD is `NotJudged`, its decimator drawing a wall of its
  own; under `HEARD_AT_LEAST` of loud audio nothing is judged. Padding is judged without the
  spectrum and makes a file `Fake` however short. Identical channels, clipped runs and a DC offset
  are findings that change no verdict.
- **The thresholds were measured on this library.** A CD rip reached past 21 kHz with no wall or
  walled at 21.4; a FLAC through LAME at 128 kbps walled at 16.6 kHz and at 320 kbps at 20.3; a copy
  sold as 96/24 walled at 21.9 kHz and read as upsampled from 44.1. Thirty real FLACs gave one fake
  (a web rip walled at 15 kHz) and two suspects at 20.0 and 20.7. **Then against every other
  encoder**: sixteen CD-rate FLACs, ninety seconds each, through ffmpeg's AAC at 128 and 256 kbps,
  libopus at 96 and 160, libvorbis at q3 and q6 and LAME V0, decoded back to 16-bit FLAC. Before
  the typical spectrum, AAC 128 was caught 13 times in 16, Vorbis q3 12, Opus 96 7 as suspect,
  Vorbis q6 3 and V0 2; after, AAC 128 15, Vorbis q3 15, Opus 96 14 and Opus 160 12 as suspect
  (Opus lowpasses at 20 kHz whatever the bitrate, landing in the suspect band), Vorbis q6 11 and
  V0 9. AAC 256 has no lowpass a wall can find and was never caught. Sixty random library FLACs read
  the same under both but three, each now suspect with a wall at 19.x or 20.x kHz, all web or scene
  rips. **What else was weighed against AAC 256, and why none of it is a finding**: twelve CD rips,
  a minute each, through ffmpeg's AAC at 256 and 320 kbps, LAME 320 and V0, libopus 160 and
  libvorbis q6. The AAC encodes keep every 500 Hz band from 14 to 22 kHz within half a decibel of
  their source; leave no more 100 Hz holes (a band 20–25 dB under its neighbours' median) than the
  source; keep the side channel above 10 kHz as loud against the mid as the source, where joint
  stereo would have zeroed it; swing the top octave window to window by the same 1–5 dB; and an
  MDCT on AAC's own 1 024-sample grid, at the decode's starting offset and three others, finds no
  more coefficients at the 16-bit floor in 11–19 kHz than the source's. Only the MP3s differ — a
  top octave swinging two to three times as far — and a wall at 19.5–20 kHz already calls every
  one. A detector unable to tell the two apart on this material would be a threshold waiting to
  misfire, so the verdict reads walls and padding alone.

## Studies in the catalog

- **`track_studies` is one row per track, and a changed file forgets its row by trigger.** It
  holds the verdict and what it rests on, levels and loudness, the print, and what the print was
  recognised as: `recognised` when asked, `heard_as` and its score, title and artist, and an
  `Agreement`. `track_studies_forget_a_changed_file` deletes the row wherever an update moves size,
  mtime or span, so the next lookup studies the file again; a rescan of an unchanged file keeps it.
  A trigger rather than an upsert clause, because every path rewriting those columns is then
  covered by existing. **A study that fails is kept as failed.** A track that will not decode wrote
  no row, so every lookup decoded it again; `unstudied` holds its id under the `JUDGED_UNDER` it
  failed at, `to_study` passes it over and the fingerprint route (`Library::will_not_study`) asks
  nothing of it, until `unstudied_forget_a_changed_file` — the same trigger on the same columns —
  sees the file change, `JUDGED_UNDER` moves, or a `refresh` asks every track again; a study that
  lands takes the mark away. A stopped study is not a failure and leaves none
  (`a_track_that_will_not_decode_is_not_decoded_again_until_it_changes_or_is_asked_again`).
- **The enrichment studies every track beside the pass.** `Studies` is a pool of
  `available_parallelism / 2` threads (at least one) named `resonate-study-<n>`, drawing off one
  shared counter, started before the pass and joined after the pictures. `to_study` is every track
  with no study, a study under an older `JUDGED_UNDER`, or a print not yet recognised — the last
  only where something can recognise it, and again under `refresh`, which asks the service again
  from the kept print rather than decoding a library twice. `at_most` caps it with the rest, and
  `EnrichOptions::studies` — the `study` key, Online's *Studying tracks* — leaves the pool
  unstarted, while the fingerprint route still studies the one track it must recognise.
- **A study reads what the player reads.** `Library::sources` is `Sources::local()` with
  `VaultFiles` and the stand-in over it wherever the library holds a vault — the binary's play
  sources — so a vaulted row whose file has gone is studied from its object, a `.wav.zst` unpacked
  on the way, and a vault-only delivered row studied like any other. Pool and fingerprint route
  both take it.
- **A track is studied once, by whoever reaches it first.** `Claims` is shared by pool and pass: a
  worker finding a track claimed passes it over, and the fingerprint route finding one claimed waits
  and reads what landed. Without it a tagged track reached by both was decoded and recognised twice.
- **Recognition is weighed against what the row is called.** `enrich::agreement` takes matches at
  `HEARD_AT_LEAST` (80): none is `Unheard`; a row whose file named no title `Unnamed`; a match on the
  row's recording id — its own `tracks.mbid` or its paired release row's — or on title and credit
  through the enrichment's `same_name` and `same_credit` `Agrees`; anything else `Disagrees`. A
  refused lookup stamps nothing, so it is asked again.
- **`Route::Fingerprint` reads the stored recognition before it asks.** With none kept it studies
  the track on the spot through the same claim, and takes a match under the strict score as
  `Certainty::Nearly`, so audio fills a name the file never gave and never overwrites one it did.
- **What the audio was heard as becomes the row's name only when a listener says so.**
  `Library::take_what_was_heard` is `enriched::land_what_was_heard`: in one transaction it writes
  the study's `heard_title` as title, `heard_artist` as artist — repointing `artist_id` through
  `store::artist_named_in` — and `heard_as` as `tracks.mbid`, *replacing* a recording id the file
  carried, since a file misnamed by its tags is misnamed by its tag ids too. It takes the rest of
  the file's word about which song it is with it — the ISRC, the release-track id, the release
  title, track and disc numbers and, where a heard artist is billed, the artist id — and lets go of
  any `release_tracks` row the track was paired to (the song the file claimed). It then rematches
  the album: where its release holds the recording the audio is, the track is seated on that row
  and takes its position; where it holds none the track stands unpaired and unnumbered and the row
  it left is listed missing, a position being what would pair it straight back. It stamps
  `answered` and clears `asked`, so a rescan of the unchanged file keeps it all (the rescan rule in
  `library.md`), and an unpaired track is due at once, which the next lookup answers through
  `Route::Recording`, landing the release the recording sits on. It marks the study `Agrees` and
  rewrites the row into `tracks_fts`. A row with no recognition kept answers `None` and writes
  nothing. A gesture rather than a rule of the pass, because a fingerprint is weighed on score
  alone and a score is no reason to rename a file somebody tagged. The Analysis pane's recognition
  card offers it as *Take this name* wherever the card leads — the audio disagreeing with the file
  or the file naming nothing — and `AnalysisModel::take_the_name` marks the card agreeing once it
  lands; `resonate studies --take <FILE>` is the same gesture, reading a file or `#frames=` URI
  through the reader `analyse` takes.
  `a_track_heard_as_another_song_takes_that_name_when_asked_and_keeps_it_through_a_rescan`,
  `a_track_heard_as_nothing_has_nothing_to_take`,
  `a_name_taken_from_the_audio_moves_the_track_to_the_row_of_the_album_that_song_is` and
  `a_name_taken_from_the_audio_lets_go_of_what_the_file_said_it_was_through_a_rescan` are the
  claims.
- **`is:fake`, `is:suspect` and `is:misnamed` are `Shape`s**, an `EXISTS` over `track_studies`, so
  the tracks pane lists every fake through the ordinary grammar.

## Recognition

- **The seam is `Fingerprints`, and a `Sounded` carries the print.** The library never prints for
  the service and the service never decodes: `Fingerprinters::recognise` hands every printer the
  print in turn and answers a `Recognition` — the matches and whether a printer refused — which the
  pane and the pass share.
- **`AcoustId` is `resonate-online`'s printer, asking with a key the listener registered.**
  `Host::AcoustId` is paced at `ACOUSTID_INTERVAL` (334 ms), the service's three requests a second.
  The lookup is a POST to `/lookup` of a form — the key, `meta=recordings`, the length and the print
  — gzipped under `Content-Encoding: gzip`, how the service takes a print too long for a query
  string; `Posted::packed_form` builds it and falls back to the plain form where packing fails,
  which the service reads as well. Read under the client's usual cap; each result's recordings
  become `RecordingMatch`es scored by the result's score, a recording answered twice kept at its
  best, one with no title dropped. The build ships no key: `acoustid-key` is empty by default and
  `online::fingerprinters` registers nothing but the stub until it is set and `online` is on.

## The command line

- **`resonate analyse` takes a cut as well as a file, read the way the queue reads it.**
  `Analysed::named` reads a URI through `from_uri_within`, so a queue row's published `#frames=`
  names the same cut here, and a `.cue` through `read_cue_media` and the sheet's own track number
  under `--track`, resolving the file as `sheet_items` does. A sheet with no `--track`, a number the
  sheet lacks and a `--track` beside a non-sheet are each an error of their own rather than a
  whole-file analysis nobody asked for.
- **The waveform is drawn as the pane draws it, mirrored about zero.** Each column is the loudest
  lane's reach above and below and its RMS, the RMS solid and the peak beyond it shaded, so a
  brick-walled master reads as a slab and a dynamic one as a thin core under tall peaks.

## The pane

- **`Pane::Analysis` follows the playing row and analyses only while in front.**
  `AnalysisModel::follow` is called from the render; a new row stops the old `Watch` and starts
  `Player::analyse` on the background executor, a `tick` notifies every `PROGRESS_EVERY` while it
  runs, and `ANALYSES_KEPT` rows are held for the run, so going back costs nothing. Condensing is
  done once, when the analysis lands — `Drawn` holds `WAVEFORM_COLUMNS` a lane and
  `SPECTRUM_COLUMNS` of spectrum — and the spectrogram is painted once per row and ramp on the
  background executor, so a frame draws polygons and an image and computes nothing.
- **What the pane learns it hands to the catalog.** `settled` writes the study where the catalog
  holds the row and none is kept — never over one, which would drop a recognition the lookup
  landed — and recognises through `Library::recognise`, so a track heard in the pane is not decoded
  again by the next lookup. A row no scan has seen is recognised through the registry directly and
  carries no agreement.
- **The recognition card leads where the audio is not the song the file names**; otherwise it
  follows the measures. A press on the waveform is `seek_to`, measured against the bounds its canvas
  recorded in prepaint.
