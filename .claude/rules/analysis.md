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
spectrum holds, whether the lossless claim its container makes holds, how loud it is and what its
audio prints as. It is a leaf beside the vault — `resonate-core`, `resonate-codec`, `resonate-dsp` for the true-peak
meter the engine's guard shares, `rustfft`, `rusty-chromaprint` and `image` — and `cargo tree -p resonate-analysis` must stay free of gpui,
the engine, the library and `ureq`. The engine re-exports its vocabulary and answers
`Player::analyse` through the player's own `Sources`, so the window reaches it through the stand-in
and a vaulted row is analysed out of its vault object; the library reaches `study` for the
enrichment's studies.

## One pass, many builders

- **`analyse` and `study` are one walk, and what they differ in is a type.** `walked` opens the
  decoder — `open_span` for a cut — at the width the vault's `pcm_of` would choose, so integer
  samples arrive with their low bits intact, and hands every block to the builders. `Drawing` is
  the seam between the two: `Unseen` for a `study` and `Drawn` — the envelope and the spectrogram —
  for an `analyse`, so a study the enrichment takes of a whole library builds neither of the two
  things only a screen wants. A DSD stream is decoded to F32 PCM through its decimator rather than
  as DoP.
- **`Watching` is how a caller stops a pass and follows it.** `Watch` is the window's — atomics for
  the frames done and expected and a stop flag — and `EnrichProgress` implements the same trait, so
  cancelling a lookup stops every study at its next block. A stopped pass is `Error::Stopped`,
  which every caller reads as no answer rather than as a failure.
- **What would grow without bound is held in `Doubling`.** A column type that `Absorbs` is pushed
  a unit at a time, and whenever the columns reach the cap every pair is merged and each column
  holds twice the units, so a stream of unknown length ends in at most `ENVELOPE_COLUMNS` or
  `SPECTROGRAM_COLUMNS` columns, every one whole but the last. `Envelope::condensed` is how a caller
  reaches any width from it, reading each output column off the frames it spans.
- **The spectrum is a Hann-windowed transform over the mono mix, sized by the rate.** 4 096 points
  at 44.1 and 48 kHz and doubling per doubling of the rate, so a bin is about 11 Hz wherever the
  stream sits; scaled so a full-scale sine on a bin reads 0 dB. The long-term average leaves out
  every window quieter than `SILENT_BELOW_DB`, because a fade and a gap between tracks would
  otherwise pull the average towards the floor it is being weighed against; `Spectrum::heard` is
  how much was loud enough to count. The spectrogram keeps each row's loudest bin, on a linear axis
  from nothing to Nyquist, because a lowpass wall is a horizontal line there and a curve on a
  logarithmic one. `Spectrogram::painted` writes it as a PNG in a `Ramp` the caller passes, so the
  theme stays in the window and the image crate out of it.
- **The bits in use are the trailing zeros of every sample ORed together.** A 16-bit master written
  into a 24-bit container leaves the low eight bits of every sample zero; a float file on the
  16-bit grid is the same thing in another shape. It is weighed beside the declared depth, never in
  place of it.
- **Loudness is BS.1770 and the DR reading is the TT meter's.** The K-weighting's two biquads are
  derived for the stream's own rate — `the_k_weighting_at_48_khz_is_the_one_the_recommendation_tabulates`
  holds the derivation to the tabulated coefficients — over 400 ms blocks at 100 ms steps, gated at
  −70 LUFS and 10 LU under the ungated mean. A channel is weighed by where its speaker sits:
  `resonate_codec::Speakers::placements` reads the positions the source names — the WAVE order
  where it names none — the LFE weighs nothing, a side speaker 1.41, a rear one 1.41 only where
  there is no side pair for it to stand behind, and every other 1.0, which is the recommendation's
  ±60° to ±120° rule read off names rather than angles. The loudness range is EBU Tech 3342's: the
  3 s short-term blocks at the same 100 ms steps, gated at −70 LUFS and 20 LU under their mean, and
  the spread from the 10th to the 95th percentile of what is left; the momentary and short-term
  maxima are the loudest 400 ms and 3 s blocks. All four read what `ffmpeg -af ebur128` reads, to
  the tenth, on a stereo master and on a 5.1 mix whose LFE carries a whole channel. DR is the second-loudest 3 s block's peak over
  the RMS of the loudest fifth, averaged over the channels and rounded. The true peak is
  `resonate-dsp`'s `TruePeakMeter` over the same blocks: every channel read at eight times its rate
  through a 32-tap windowed-sinc interpolator, all eight phases accumulated in one pass, and the
  loudest of them kept — the reading the engine's guard is built on, so the study and the playback
  cannot disagree about what an over is. It is `Loudness::true_peak` and
  `track_studies.true_peak`, and it is what `Library::hinting` hands the player — beside the
  ReplayGain the integrated loudness asks for, which a track whose tags declare none is levelled
  by (`audio.md` has the rule).
- **The print is Chromaprint's own algorithm, and only the first two minutes of it.**
  `rusty-chromaprint` under `Configuration::preset_test2` — the preset libchromaprint and AcoustID
  default to — fed 16-bit samples for `PRINTED_FOR`, compressed and written in URL-safe base64
  without padding, which is what AcoustID reads. `resonate_core::Chromaprint` carries it with the
  whole track's length, because that is the duration a lookup is asked with. A stream the printer
  refuses to start is a debug record and `print: None`, never a failed analysis.
- **A clip is printed and signed as well as a track.** `print_clip` runs the same `Printing` over
  a buffer a listener recorded, so AcoustID is asked about a snippet with the print it would be
  asked about a file with. `signature_of` is Shazam's signature, written here from the format as
  it is documented rather than from any client's source: the clip mixed to mono, resampled to
  16 kHz through `resonate-dsp`'s `Resampler` at `Balanced`, the middle twelve seconds kept and
  scaled to the 16-bit range; a 2 048-point Hann transform every 128 samples, its power over 2¹⁷
  held in a ring of 256 frames beside a spread that takes each bin's maximum over it and the next
  two and folds into the frames one, three and six back; and a peak read 46 frames behind the
  newest where the power reaches 1/64, the spread just below it, eight neighbouring bins of the
  spread 49 frames back and fourteen other frames of it, its magnitude `max(ln p, 1/64)·1477.3 +
  6144` and its bin refined to a 64th by the parabola through its neighbours. Peaks are filed into
  250–520, 520–1 450, 1 450–3 500 and 3 500–5 500 Hz and written as the service reads them: a
  48-byte header of `0xcafe2580`, a CRC-32 of everything past the eighth byte, the size, the
  16 kHz rate id shifted by 27 and the sample count plus a quarter-second's worth, then each band's
  tag, length and peaks — a frame delta, escaped with `0xff` and the whole frame where it will not
  fit a byte, then the magnitude and the bin — padded to four bytes, and handed on as a
  `data:audio/vnd.shazam.sig;base64,` URI. A steady tone signs as nothing, because a peak has to
  stand out in time as well as frequency; `a_tone_leaves_its_peaks_in_the_band_it_sounds_in` plays
  notes that start and stop.

## The verdict

`verdict.rs` is pure: a `Weighed` of the codec, the rate, the declared depth, the spectrum and the
levels in, a `Judgement` out. `JUDGED_UNDER` names the heuristic and is bumped by hand whenever what
it answers could change, because it is what a stored study is weighed against.

- **A wall is a steep drop to a floor that stays there.** The spectrum is read in 100 Hz bands up
  to `TOP_GUARD` of Nyquist. An edge is a wall where the mean of the bands from a kilohertz to
  200 Hz below it stands at least `WALL_DB` over the mean of every band from 200 Hz above it to the
  top, *and* at least `WALL_DB` less `WALL_FLOOR_SLACK_DB` over the loudest of them — the second
  condition is what keeps a gentle slope from reading as a wall, because a slope's loudest band
  above the edge is the one just past it. The highest run of walled edges is taken and the steepest
  in it is the cutoff, so a wall is found where content ends rather than where it first thins.
- **Where the wall stands is what it means.** At or under `LOSSY_CEILING_HZ`, 19.5 kHz, it is where
  a lossy encoder cuts and the file is `Fake`, with a `LossyGuess` read off LAME's own lowpass
  table; up to `SUSPECT_CEILING_HZ`, 20.7 kHz, it is `Suspect`, the band where a high-bitrate
  encode and a genuinely low anti-alias filter cannot be told apart; above it, it is an anti-alias
  filter. A stream above 48 kHz is weighed against the rates it could have been upsampled from —
  only those at most half its own, so a 96 kHz file is never called an 88.2 kHz one upsampled —
  and a wall standing no higher than one of their Nyquists and `UPSAMPLE_SLACK_HZ` over it is
  `Upsampled`, which is `Fake`.
- **No wall is not a pass.** A spectrum that reaches past `SUSPECT_CEILING_HZ` — or past where the
  highest rate it could have been upsampled from would have ended, for a hi-res stream — within `CONTENT_WITHIN_DB` of
  its 1 to 4 kHz level is `Genuine`; one that fades out below that with no wall is `NotJudged`,
  because an old recording and a band-limited master fade the same way a transcode does not.
- **What cannot be judged by the spectrum is not.** A lossy codec is `Lossy` whatever its spectrum
  says, and still reports the wall with a guess; DSD is `NotJudged` because its decimator draws a
  wall of its own; under `HEARD_AT_LEAST` of loud audio nothing is judged. Padding is judged
  without the spectrum and makes a file `Fake` however short it is. Identical channels, clipped
  runs and a DC offset are findings that change no verdict.
- **The thresholds were measured on this library.** A CD rip reached past 21 kHz with no wall or
  walled at 21.4; a FLAC put through LAME at 128 kbps walled at 16.6 kHz and at 320 kbps at
  20.3; a copy sold as 96/24 walled at 21.9 kHz and read as upsampled from 44.1. Thirty real FLACs
  gave one fake — a web rip walled at 15 kHz — and two suspects at 20.0 and 20.7.

## Studies in the catalog

- **`track_studies` is one row per track, and a changed file forgets its row by trigger.**
  It holds the verdict and what it rests on, the levels and the loudness, the print, and what the
  print was recognised as: `recognised` when it was asked, `heard_as` and its score, title and
  artist, and an `Agreement`. `track_studies_forget_a_changed_file` deletes the row wherever an
  update moves the size, the mtime or the span, so the next lookup studies the file again; a
  rescan of an unchanged file keeps it. A trigger rather than a clause in the upsert, because every
  path that rewrites those columns is covered by existing.
- **The enrichment studies every track beside the pass.** `Studies` is a pool of
  `available_parallelism / 2` threads named `resonate-study-<n>` drawing off one shared counter,
  started before the pass and joined after the pictures. `to_study` is what they are given: every
  track with no study, a study under an older `JUDGED_UNDER`, or a print not yet recognised — the
  last only where something can recognise it, and again under `refresh`, which asks the service
  again from the kept print rather than decoding a library twice. `at_most` caps it with the rest,
  and `EnrichOptions::studies` — the `study` key and the Online category's *Studying tracks* —
  leaves the pool unstarted, while the fingerprint route still studies the one track it has to
  recognise.
- **A study reads what the player reads.** `Library::sources` is `Sources::local()` with
  `VaultFiles` and the catalog's stand-in over it wherever the library holds a vault — the same
  sources the binary plays through — so a vaulted row whose file has gone is studied out of its
  object, a packed `.wav.zst` is unpacked on the way, and a delivered row that lives only in the
  vault is studied like any other. The pool and the fingerprint route both take it.
- **A track is studied by whoever reaches it first, and only once.** `Claims` is shared by the pool
  and the pass: a worker that finds a track claimed passes it over, and the pass's fingerprint route
  that finds one claimed waits for it and reads what landed. Without it a tagged track reached by
  both was decoded twice and recognised twice.
- **Recognition is weighed against what the row is called.** `enrich::agreement` takes the matches
  at `HEARD_AT_LEAST`, 80: none is `Unheard`, a row whose file named no title is `Unnamed`, a match
  on the row's recording id — its own `tracks.mbid` or the one its paired release row carries — or
  on the title and credit through the enrichment's own `same_name` and `same_credit` is `Agrees`,
  and anything else is `Disagrees`. A refused lookup stamps nothing, so it is asked again.
- **`Route::Fingerprint` reads the stored recognition before it asks.** Where none is kept it
  studies the track on the spot through the same claim, and it takes a match under the strict
  score as `Certainty::Nearly`, so audio fills a name the file never gave and never overwrites one
  it did.
- **`is:fake`, `is:suspect` and `is:misnamed` are `Shape`s**, an `EXISTS` over `track_studies`, so
  the tracks pane lists every fake in the library through the ordinary grammar.

## Recognition

- **The seam is `Fingerprints` and a `Sounded` carries the print.** The library never prints on the
  service's behalf and the service never decodes: `Fingerprinters::recognise` hands every printer
  the print in turn and answers a `Recognition` — the matches and whether a printer refused — which
  the pane and the pass share.
- **`AcoustId` is `resonate-online`'s printer, and it asks with a key the listener registered.**
  `Host::AcoustId` is paced at `ACOUSTID_INTERVAL`, 334 ms, the service's three requests a second.
  The lookup is a POST to `/lookup` of a form — the key, `meta=recordings`, the length and the
  print — gzipped under `Content-Encoding: gzip`, which is how the service asks for a print too
  long to sit in a query string; `Posted::packed_form` builds it and falls back to the plain form
  where the packing fails, which the service reads just as well. It is read under the client's
  usual cap, and each result's recordings become `RecordingMatch`es scored by the result's score, a recording answered twice
  kept at its best, and a recording with no title dropped as nothing a listener could read. The
  build ships no key: `acoustid-key` is empty by default and `online::fingerprinters` registers
  nothing but the stub until it is set and `online` is on.

## The command line

- **`resonate analyse` takes a cut as well as a file, and reads it the way the queue does.**
  `Analysed::named` reads a URI through `from_uri_within`, so the `#frames=` a queue row is
  published under names the same cut here, and a `.cue` through `read_cue_media` and the sheet's
  own track number under `--track`, resolving the file the way `sheet_items` does. A sheet with no
  `--track`, a number the sheet does not hold and a `--track` beside anything that is not a sheet
  are each an error of their own rather than a whole-file analysis nobody asked for.
- **The waveform is drawn as the pane draws it, mirrored about zero.** Each column is the loudest
  lane's reach above and below and its RMS, the RMS solid and the peak beyond it shaded, so a
  brick-walled master reads as a slab and a dynamic one as a thin core under tall peaks.

## The pane

- **`Pane::Analysis` follows the playing row and analyses only while it is in front.**
  `AnalysisModel::follow` is called from the render; a new row stops the old `Watch` and starts
  `Player::analyse` on the background executor, a `tick` notifies every `PROGRESS_EVERY` while it
  runs, and `ANALYSES_KEPT` rows are held for the run so going back to a track costs nothing. The
  condensing is done once, when the analysis lands — `Drawn` holds `WAVEFORM_COLUMNS` a lane and
  `SPECTRUM_COLUMNS` of spectrum — and the spectrogram is painted once per row and ramp on the
  background executor, so a frame draws polygons and an image and computes nothing.
- **What the pane learns it hands to the catalog.** `settled` writes the study where the catalog
  holds the row and none is kept — never over one, which would drop a recognition the lookup
  landed — and recognises through `Library::recognise`, so a track heard in the pane is not decoded
  again by the next lookup. A row no scan has seen is recognised through the registry directly and
  carries no agreement.
- **The recognition card leads where the audio is not the song the file names.** Otherwise it
  follows the measures. A press on the waveform is `seek_to`, measured against the bounds its
  canvas recorded in prepaint.
