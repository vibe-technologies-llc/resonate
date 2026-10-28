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

`resonate-analysis`: one whole decode → envelope, spectrum, lossless-claim verdict, loudness, print.
Leaf beside the vault (core, codec, dsp, `rustfft`, `rusty-chromaprint`); free of gpui, engine,
library, `ureq`. `Player::analyse` uses the player's own `Sources` (vaulted rows analyse from their
object). Library reaches `study` (enrichment) and `print` (moved files).

## One pass, many builders

- **`analyse` and `study` are one walk, differing in a type.** `walked` opens the decoder at the
  width the vault's `pcm_of` picks. The `Drawing` trait is `Unseen` (`study`) or `Drawn` (envelope,
  spectrogram; `analyse`), so library-wide studies build nothing only a screen wants. DSD → F32 via
  its decimator, not DoP.
- **A stream that turns is brought back to the shape it opened in.** `study`, `analyse`, `print` and
  `excerpt` follow format changes (`Decoder::follow_format_changes`) through `conform::Walking`: on
  each `Reformatted` the previous segment's resampler is flushed, and the new shape gets a `Remix`
  to the opening layout and a `Resampler` (`Quality::Balanced`, linear phase) to the opening rate
  from `resonate-dsp`, so every builder, the frame count and `Examined` keep one rate and one
  channel count and the length is on the opening clock, as `Decoder::position` is. A shape equal to
  the opening one passes through untouched. A ratio the resampler refuses is `Error::Conformity`.
  The native-sample readings do not survive a conversion: once a stream has turned, `Leveling`
  reports no `bits_in_use` and no `stereo` (padding and identical channels are claims about one
  shape's own samples), while peak, RMS, DC, clipping, loudness, spectrum and print read the
  conformed signal. The study's true peak is the opening shape's meter over that signal; playback's
  guard and the engine's `measure::Measuring` meter each shape as it sounds
  (`a_stream_that_turns_is_as_loud_as_the_same_tone_that_never_does`,
  `a_study_of_a_stream_that_turns_covers_every_link_in_the_shape_it_opened_in`).
- **`peak_through` is a decode of its own, not part of a study.** It walks like `print` and `excerpt`
  (`Walking`, `follow_format_changes`) but runs the room correction's `Convolver` (where one is applied, its tail flushed through the
  stage once the song ends) and the equaliser stage, then a true-peak meter, instead of the
  builders, stores nothing and is asked on a press for the songs no study has measured
  (`eq.md`, *Fit the preamp to the music*).
- **A study keeps where the music sounds, per band.** `BandLevels::of(&spectrum)` sums the study's
  per-bin mean power into 29 third-octave bands (`BANDS`; edges at 25 Hz times `2^(n/3)`, band 0
  taking everything under 31.5 Hz and the last everything above its edge, a bin read at its own
  frequency and DC left out) and keeps each as a byte, half-dB steps below the loudest band
  (`SILENT` where the band held nothing above `FLOOR_DB`, so a silent song is all `SILENT`). It is
  derived from the spectrum, so a kept analysis, a study and `band_figures(sources, location, span,
  watch)` (one decode with the transform and the bank, for a study kept without them) agree
  (`the_bands_measured_alone_are_the_bands_a_study_keeps`); `track_studies.band_levels` holds the 29
  bytes and a row of any other length reads as unmeasured, so a later layout re-measures instead
  of misreading. The edges are absolute hertz, so a measure is good at any stream rate.
- **A study keeps each band's own peak beside its level, over moments.** `split::Splitting` runs in
  the study's decode (and in `band_figures`): per channel, 28 zero-phase low-pass filters, one at
  each band edge (four Butterworth biquads, an eighth-order Butterworth, run forward and then
  backward over the same samples, `1 / (1 + (f/fc)^16)` as the response, real and never
  negative), whose differences are the 29 bands, the first being the lowest low-pass and the last
  the signal less the highest. The bands therefore sum to the signal exactly, whatever the
  filters, and a tone is shared between its neighbours without any of it counted twice. The
  forward pass carries its state through the whole stream; the backward pass needs the future, so
  the stream is settled half a second at a time, each pass starting a fifth of a second past what
  it settles (the lowest edge's slowest pole has fallen to under a thousandth by then), and the
  first and last fifths are the stream mirrored (a zero extension would put a step in the lowest
  bands; an odd mirror a bias in them); a stream noted in pieces gives the peaks it gives whole
  (`a_stream_noted_in_pieces_has_the_peaks_it_has_noted_whole`). **The band tree**
  (`peaks::TREE`): 57 nodes, the whole signal first, then each node's bands halved down to each
  band alone, every node a contiguous run of bands, so its signal is the difference of two edge
  filters. **Tiles**: every twentieth of a second (doubling, pairs merged, past 16 384 tiles) keeps
  each node's peak, read at every sample and at the half-sample between (a cubic midpoint) of
  every channel, the loudest kept, then divided by the least a tone at the reach of the node's top
  band could show to those probes (`shortfall`), so the figure is the continuous peak or over it;
  the whole signal's peak is instead the true peak of the tile (`TruePeakMeter::at`, the decode's
  own meter, taken a tile at a time). **Moments** (`peaks::grouped`): each tile is widened to the
  loudest of each node over its two neighbours each way (a tenth of a second, what the fit's
  `G_soon` spans, `eq.md`), the tiles are paired down to at most 1 024, then gathered loudest
  first: a tile joins the moment it raises least (the largest rise of a node over the moment's,
  relative to the moment's, times the tile's whole peak over the song's), or starts one while
  fewer than `MOMENTS_KEPT` (8) stand; the tolerance a tile may raise a moment by is bisected to
  the least that leaves at most eight. A moment is the loudest of each node over its tiles, so
  every tile is held by one, and moments kept apart are moments that did not sound together
  (`a_burst_is_kept_apart_from_a_tone_heard_long_after_it`,
  `moments_far_apart_in_time_and_in_kind_are_kept_apart_and_quiet_ones_join_the_rest`). Eight
  were chosen by measure: on four twenty-second excerpts of released songs under eight curves the
  bound sat 3.55 dB over the decode on average with one moment, 3.35 with four, 3.28 with eight,
  3.23 with sixteen and 3.15 with thirty-two, under a tenth of a decibel for each doubling past
  eight. **Kept**: each moment is its loudest node as a `u16`
  (1/16 dB steps from -200 dBFS, rounded up) and each node a byte of quarter-decibel steps below it
  (rounded down, so the figure read is never under the one measured; 254 holds anything deeper
  than 63.5 dB below, 255 a node that held nothing); the moments loudest first, silent moments
  after: 472 bytes (`BAND_PEAKS_BYTES`), `track_studies.band_peaks`, a row of any other length
  reading as unmeasured. `Study::band_peaks` and the kept analysis (`WRITTEN_AS` 5) carry it, so a
  study from a recalled analysis keeps its peaks. `BandPeaks::at_once` makes the peaks of bands
  heard as one moment (each node the sum of its bands), for a caller that has only band peaks. The
  bank, the tree and the moments cost about 14 ms of one core a second of 44.1 kHz stereo audio,
  twice the 7 ms the one-moment bank of two biquads an edge measured the same way. `eq.md` has the bound it makes of them.
- **Non-number samples are silence**: `normalised` maps float NaN/infinity to 0 before any builder
  (one would ride the K-weighting history to track end). Bit reading keeps native samples.
- **`Watching` stops a pass.** `Watch` is the window's; `EnrichProgress` implements it, so cancelling
  a lookup stops every study at its next block. `Error::Stopped` is no answer, not a failure.
- **`Doubling` bounds growth**: at the cap every column pair merges (`ENVELOPE_COLUMNS`,
  `SPECTROGRAM_COLUMNS`).
- **Spectrum: Hann transform of every channel but LFE**, bin = mean of channel powers. Not a mono mix
  (cancels opposite-phase stereo to silence → `NotJudged`; adds LFE rumble). Average skips windows
  under `SILENT_BELOW_DB`; `Spectrum::heard` = amount counted. Spectrogram axis linear;
  `Spectrogram::painted` writes a `Raster` through the caller's `Ramp` (theme stays in the window).
- **Bits in use = trailing zeros of all samples ORed**, beside declared depth, never instead; float
  depth = `BITS_A_FLOAT_CARRIES`.
- **Loudness BS.1770, range EBU Tech 3342, DR the TT meter's.** K-weighting derived per stream rate;
  channel weight by placement (`resonate_codec::Speakers::placements`, WAVE order if source names
  none): LFE 0, side `SURROUND_WEIGHT`, rear `SURROUND_WEIGHT` only with no side channel, rest 1.0.
  Matches `ffmpeg -af ebur128` to the tenth, 5.1 included (`tests/references.rs`: stereo at 44.1, 48, 88.2, 96 and 192 kHz and 5.1 with rears or sides against ffmpeg, EBU Tech 3341's tone and gating cases, Tech 3342's range steps, a print at Chromaprint's own 11 025 Hz equal to `fpcalc`'s). True peak = `TruePeakMeter::at(rate, ..)` (the engine
  guard's own interpolator, folding 4×, 2× or not at all by rate as the guard does, so study and
  playback agree about an over; `TruePeakMeter::new` is the 8× read the visualiser's meter has no rate for), stored `track_studies.true_peak`, handed
  to the player by `Library::hinting` (`audio.md`).
- **Print = Chromaprint, first `PRINTED_FOR` only**: `rusty-chromaprint`
  `Configuration::preset_test2` (the AcoustID default), URL-safe base64, unpadded.
  `resonate_core::Chromaprint` carries it with whole-track length; lookups ask with the track's
  known length, else the print's. A printer refusing a stream: debug record, `print: None`, never a
  failed analysis.
- **A clip is signed, never printed**: AcoustID filters by a whole recording's `duration`, matches
  from its start. Listen asks Shazam and AudD; AcoustID only a file printed whole. `signature_of`
  writes Shazam's signature from the documented format: mono, 16 kHz via `Resampler` at `Balanced`,
  at most `SIGNED_SECONDS_AT_MOST` from the middle, as a `data:audio/vnd.shazam.sig;base64,` URI.
  `Signature::is_worth_asking` holds it to `FEWEST_PEAKS_A_SECOND`: `Shazam::signed` asks nothing
  about a steady tone or lone click, for Listen and `ByEar` alike.

## What the window drew is kept

**Per-file, forgotten when the file changes.** `KeptAnalyses`: one file each in
`$XDG_CACHE_HOME/resonate/analyses` (given to the window's `Player` by `Player::keeping_analyses`),
named by FNV-1a of path, span, size, mtime; where only a vault object serves the row, path and span
(`NO_FILE_STANDS`). `Player::analyse` asks it before decoding and keeps what it decoded. Another
`JUDGED_UNDER` stamp reads as nothing; judgement is unwritten (`read` runs `judged` over the kept
spectrum, levels and `Steps`). Reader trusts no shape: envelope with no lanes, over `ENVELOPE_LANES`, or
columns of no frames → nothing. Unreadable files are deleted; writes staged (`<pid>-<n>.staged`,
under `writer::Held`) + renamed; `trim` sweeps what a dead writer staged, counts what a living one is
writing, and drops least lately used past `KEPT_BYTES_AT_MOST`
(`what_a_dead_writer_staged_is_swept_and_a_living_ones_is_counted_and_left`). Cache failure = debug
record + decode. Headless commands keep nothing.

## The verdict

`verdict.rs` is pure: `Weighed` (codec, rate, declared depth, float flag, spectrum, levels, every
grid's `Steps`) →
`Judgement`: `Verdict` (`Genuine`, `Suspect`, `Fake`, `Lossy`, `NotJudged`) + typed `Finding`s.
`JUDGED_UNDER` names the heuristic and starts at 1 with 0.1.0 (as the kept analysis's `WRITTEN_AS`
and the library's baseline do); once released, bump it by hand when its answer or a stored measure
could change (stored studies and kept analyses are weighed against it).

- **Wall = steep drop to a floor that stays.** 100 Hz bands up to `TOP_GUARD`. An edge is a wall
  where the mean of the bands 1 kHz..200 Hz below is `WALL_DB` over the mean from 200 Hz above to
  the top, *and* `WALL_DB` less `WALL_FLOOR_SLACK_DB` over the loudest of those (a gentle slope isn't
  one). Cutoff = steepest edge of the highest run of walled edges.
- **Looked for in the average and in what a quarter of windows stay under**: in loud masters decoded
  from lossy, clipping splatter fills the average above the lowpass while the wall stays in every
  unclipped window. `Spectrum::typical` = each band's `TYPICAL_QUANTILE` off a per-band histogram;
  `judged` runs `wall_in` on both, `lower_of` takes the lower
  (`a_transcode_splattered_above_its_wall_by_clipping_is_still_fake`).
- **Position = meaning.** At/under `LOSSY_CEILING_HZ`: lossy cut → `Fake` + `LossyGuess` (LAME's
  lowpass table). Up to `SUSPECT_CEILING_HZ`: `Suspect` (high-bitrate encode ≈ low anti-alias
  filter). Above: anti-alias filter. Above 48 kHz, weigh against rates it could be upsampled from
  (at most half its own): wall within `UPSAMPLE_SLACK_HZ` of one's Nyquist = `Upsampled` = `Fake`.
  Under 44.1 kHz the ceilings are unreachable: wall at/above `ANTI_ALIAS_EDGE` of Nyquist = the
  converter's (`Genuine`, no guess); lower → lossy rule; no wall: content reaching the edge
  `Genuine`, else `NotJudged`
  (`a_lossless_file_at_a_low_rate_is_judged_against_its_own_anti_alias_edge`).
- **No wall is not a pass.** Content past `SUSPECT_CEILING_HZ` (hi-res: past where the highest
  possible source rate would end) within `CONTENT_WITHIN_DB` of the 1-4 kHz level: `Genuine`; fading
  below that: `NotJudged` (old/band-limited masters fade like transcodes).
- **Unjudgeable stays unjudged.** Lossy codec: `Lossy` whatever the spectrum, wall still reported.
  DSD: `NotJudged` (decimator draws its own wall). Under `HEARD_AT_LEAST` (5 s) of loud audio:
  nothing judged. Padding judged without the spectrum → `Fake`. Identical channels, clipped runs, DC
  offset: findings only.
- **A decode of a lossy codec lands on that codec's quantiser steps** (`steps.rs`, `granules.rs`,
  `weighing.rs`; `Finding::QuantiserSteps { coding, resampled_from, lattice }` → `Fake`). AAC and
  MP3 store each transform coefficient as `sign · q^(4/3) · 2^(k/4)` (AAC's scale factor, MP3's
  global gain, scale factors and subblock gain are all quarter-octave steps), and Vorbis as a small
  integer residue times a floor read from `FLOOR1_inverse_dB_table`, a geometric table of
  `VORBIS_FLOOR_OCTAVES_A_STEP` (0.0908) octaves a step. Re-taking the encoder's own transform on
  its own grid gives back the coefficients the decoder summed, 16-bit rounding aside, so the
  phase of `log2|c|` against that step sits at one place for the smallest levels whatever the
  band's gain, where a recording's coefficients spread over the circle; the mean of the phases
  (the *lattice*) measures it. `Phases` looks the phase up from a float's mantissa (top
  `MANTISSA_BITS`) and exponent. Vorbis is read at `VORBIS_TURNS_A_FLOOR_STEP` (2) turns a floor
  step, the second harmonic, since `log2(r)/0.0908` is near a whole turn for residues 1, 2 and 4
  and near a half for 3, 5 and 6 (q10 read 0.0131 on the fundamental, 0.035 on the harmonic).
  First two channels, lossless codecs only.
- **A coefficient counts by how far it stands above the 16-bit rounding.** Each carries a weight
  that rises as a smoothstep in `log2|c|` over a `Ramp` (`unit` the grid's 16-bit step in its own
  coefficients, `from` and `to` in octaves above it: AAC and Vorbis 3 to 7, MP3 2 to 6 with a unit
  of 0.535), so rounding noise and the dither on it (a triangular one at 16 times the usual scale
  still leaves the lattice) weigh nothing and a loud coefficient, whose rounding is a small share
  of its size, weighs one. The lattice is `|Σ w·phase| / Σ w`, and `Steps::coefficients` is the
  *effective* count `(Σ w)² / Σ w²`. A smooth ramp has no edge in the lattice's period, which a
  hard floor does: a floor of 8 steps biased quiet genuine tracks to 0.03-0.09 at every offset.
  Digital silence (`SILENT_COEFFICIENT_UP_TO`, 1/4 096 of a 16-bit step) carries no weight. The
  weights lifted the lattice where it had been diluted by rounding noise (faac at 64 kbps from 3
  to 12 of 36 found on the ramp alone, before the kinds) and left every genuine source at 0.01 at
  most.
- **A frame may be any block the encoder chose, so each is taken as the strongest of its kinds.**
  `Grid::KINDS` blocks are tried at every frame and the one with the largest `|Σ|² / Σ w²`
  supplies the frame's sum. A choice made on magnitude alone leaves the sum of a recording
  zero-mean (the phase is untouched), so the null only grows about 1.4 times in noise, while the
  lattice stops being diluted by frames of another shape: faac and fdk AAC take eight short blocks
  in three of four frames of a loud master (one measured stretch of 300 frames: 14 long, 30 start,
  30 stop, 226 eight short), which is why the long-block lattice of faac at 160 kbps read 0.001 on
  10 of 36 sources and the same sources read 0.24 to 0.78 with every kind. `Aac` has four kinds
  (long, start, stop, eight short blocks of 128 lines with a 256 window at 448 + 128k, the short
  window's slope the long window's family, KBD alpha 6 in the short KBD family) under both window
  families; `Mp3` has four (normal, start, stop from the standard's windows, and three 12-sample
  windows of 6 lines in each subband, with no alias butterflies and a gain of 3 so the lattice
  stays in phase with the long blocks, since the decoder's unscaled inverse transform gains
  `n / 4` for `n` 36 and 12); Vorbis has one. A control that lands on the short-block grid would
  read the lattice again (a half frame, 512, is four short hops), so `Aac`'s control is 576 off.
  Verified on synthetic streams through long, start, stop and eight short blocks in AAC and every
  block type in MP3 (`coefficients_on_the_steps_are_found_through_long_start_stop_and_eight_short_blocks`,
  `lines_on_the_steps_are_found_whatever_block_types_the_granules_run_through`).
- **One search and confirmation, one `Grid` per coding.** `Stepping<G: Grid>` holds the stage
  machine; a grid names its hop, the residues searched, the span a frame reads, its lines, its
  window shapes, its block kinds, the control's shift, the frames a search and a confirmation take
  and whether mid and side are weighed. Neither the frame offset nor the window is known (a decoder
  trims its priming, a cut starts anywhere), so the search tries every residue under every shape
  over `FRAMES_A_SEARCH` frames at `SEARCHES` (4) busy places `SEARCHED_EVERY_SECONDS` apart (a place
  with under `BUSY_SHARE` of its coefficients counted is skipped, at most
  `TRIED_FOR_EACH_SEARCH_AT_MOST` places a search tried), ranks by `|Σ|²/Σw²`, then confirms the
  best `CANDIDATES` over `CONFIRMED_FRAMES` together with a *control* `CONTROL_SHIFT` off the same
  grid.
  - `Aac`: 1 024-line MDCT, sine and Kaiser-Bessel (α 4) windows, every 1 024 residues, 4..16 kHz
    in L, R, mid and side (a band may be coded either way), 4 frames a search, 400 confirmed,
    control 576 off, rates up to 96 kHz.
  - `Mp3` (`granules.rs`): the hybrid filterbank of ISO 11172-3 run forward: the polyphase
    analysis with the standard's window (`SYNTHESIS_WINDOW`, Table B.3's integers over 65 536,
    divided by 32; the 64-row matrixing folded to 32 terms), the 18-line MDCT of 36 subband samples
    under the block window with odd subbands' odd samples inverted, then the encoder's
    alias-reduction butterflies on the long kinds (without them the lattice drops to a third).
    576 residues, 6..20 kHz in L, R, mid and side (MP3's mid and side are `/√2`, two quarter-octave
    steps from ours), 4 granules a search, 400 confirmed, control half a granule off, rates up to
    48 kHz. Only the subbands the band reaches (and one either side for the butterflies) are
    computed; polyphase vectors are kept by position (`AHashMap`, cleared after each search and
    run, at most `VECTORS_KEPT_AT_MOST`) since neighbouring residues, granules and kinds share 35
    of 36.
  - `Vorbis`: 2 048-sample blocks under Vorbis's power-sine window, L and R only (square-polar
    coupling is lossless on residues, so each channel is floor times integer). A short block run
    shifts the long-block grid by a multiple of 128 (a long-short hop is 576, short-short 128), so
    the grid is stepped every 128 samples over 128 residues: one frame in eight lands on a long
    block, which dilutes the lattice but keeps the grid across transients. 4..16 kHz, 32 frames a
    search, 1 600 confirmed, control 64 samples off, rates up to 48 kHz.
- **A decode taken to another rate is weighed brought back.** `Weighing` runs the three grids at
  the stream's rate and feeds the first two channels through a `Resampler` (`Quality::Balanced`,
  linear phase) to each rate in `BROUGHT_BACK_TO`, running `Aac` and `Mp3` there at
  `SEARCHES_RESAMPLED` (2) places: 44.1 kHz to 48, 48 to 44.1, and 88.2, 96, 176.4 and 192 kHz to
  both CD rates (a lossy CD-rate stream played out at a hi-res rate keeps its grid however the
  upsampler's wall was hidden). A linear-phase round trip puts the grid back on whole samples and
  its passband leaves 4..20 kHz as it was: ffmpeg's AAC at 256 kbps read 0.364 decoded and 0.364
  brought back from 48 kHz. Each grid stops taking samples once done, so only the first 20 s or so
  are resampled. A `Steps` carries its `coding` and the `rate` its grid stood at; a rate other than
  the stream's is `resampled_from`. A decode downsampled below the rate it was coded at has lost
  the band above the new Nyquist and the grid with it: fdk AAC at 160 kbps taken to 32 kHz is found in none.
- **Taken at a lattice per coding, over its control, over enough coefficients.** The verdict takes
  a grid at `AAC_LATTICE_AT_LEAST` (0.02), `MP3_LATTICE_AT_LEAST` (0.02) or
  `VORBIS_LATTICE_AT_LEAST` (0.012), `LATTICE_OVER_ITS_CONTROL_AT_LEAST` (4) times its control,
  `STEPPED_COEFFICIENTS_AT_LEAST` (20 000 effective) and `LATTICE_SIGNIFICANCE_AT_LEAST` (12) over
  `1 / √coefficients`: a periodic or synthetic signal that happens to cluster does so at every
  offset, which the control refuses, and a lattice among few coefficients is chance, which the
  significance refuses (a genuine track read 0.019 on 21 000 effective coefficients, a significance
  of 2.8; a real MP3 rip reads 0.023 on 300 000, 12.6). Every grid's `Steps` is kept in the study
  and the kept analysis (`WRITTEN_AS` 5, the peaks over moments after the print; 4: one moment; 3: a count, then coding name,
  rate, lattice, control and coefficients each, the last now the effective count); `track_studies`
  stores only the verdict (no basis column), so the change moved `JUDGED_UNDER` to 4 and every
  study is weighed again. Cost: about 2 s a track over the decode (3.9 s to 5.9 s for a 3:33 FLAC
  on a loaded machine, 0.9 s to 3.3 s for 40 s at 96 kHz, which runs two resamplers), fixed
  whatever its length, since every grid is done within its first 20 s
  (`coefficients_on_the_quantiser_steps_are_found_at_any_frame_offset_and_free_ones_are_not`,
  `lines_on_the_quantiser_steps_are_found_through_the_hybrid_filterbank_at_any_offset`,
  `residues_on_vorbis_floor_are_found_on_any_short_block_grid_and_free_ones_are_not`, and in
  `tests/references.rs` the ffmpeg AAC, faac and fdkaac, dithered and hi-res, resampled AAC, LAME
  and libvorbis round trips beside
  `loud_noise_and_dense_music_at_every_cd_rate_land_on_no_codecs_steps`).
- **What the steps were measured on.** 36 CD-rate library sources (28 at 44.1 kHz, 8 at 48), 40 s
  each, encoded and decoded back to 16-bit FLAC; the table is the sources found before, then now.
  ffmpeg's AAC at 256 kbps 36 and 36 (lattice 0.16 to 0.62), at 128 kbps 35 and 35; faac `-b 64` 3
  and 34, `-b 96` 21 and 36, `-b 128` 27 and 36, `-b 160` 26 and 36 (0.24 to 0.78), `-q 100` 15 and
  29, `-q 200` 26 and 34, `-q 300` 25 and 36; fdk AAC `-b 128000` 34 and 36, `-b 160000` 36 and 36,
  `-b 256000` 35 and 35, `-b 320000` 29 and 31, VBR mode 1 25 and 36, mode 3 21 and 36, mode 5 36
  and 36; LAME at 320 kbps with `--lowpass -1` 36 and 36 (0.08 to 0.26), `-V0` 36 and 36, `-V2
  --lowpass -1` 36 and 36, 192 kbps 36 and 36, 128 kbps 36 and 36 (0.48 to 0.85); libvorbis q3 34
  and 36, q6 34 and 36, q10 28 and 32, ffmpeg's Vorbis encoder 34 and 36. Taken to another rate (15
  sources, fdk AAC 160 kbps, faac 160 kbps, LAME 192 kbps): to 96 kHz 0 and 15 of 15 for each
  (lattice 0.18 to 0.54 for fdk, 0.15 to 0.70 for faac, 0.42 to 0.59 for LAME), fdk to 88.2 kHz 0
  and 15; triangular dither 15 and 15 and noise-shaped dither (shibata) 15 and 15 for fdk and LAME,
  11 and 15 for faac; a triangular dither at 16 times its scale 12 and 14 for fdk and 13 and 13 for
  LAME; fdk taken to 32 kHz 0 and 0. Every control read 0.015 at most. The sources read at most
  0.0078 (AAC), 0.0101 (MP3) and 0.0030 (Vorbis) with the weights and every kind (0.0025, 0.0035 and
  0.0017 before). The misses that remain are fdk at 320 kbps (5 of 36), faac at `-q 100` (7), `-q
  200` (2) and `-b 64` (2), libvorbis q10 (4) and a few of the others (one or two of 36), loud
  masters whose lattice reads 0.012 at most; the cause is not found (at 320 kbps the quantised
  values run large and a log lattice thins as `q` grows, `q^(4/3)` stepping by under a percent past
  `q` 100, which does not explain faac at low quality); none is a false call. The whole library, 462
  tracks studied whole (461 FLAC and a WAVE), reads at most 0.0194 on a genuine track with 20 000
  effective coefficients or more (a significance of 2.8, at most 4.95 anywhere) and no verdict
  moved: 9 tracks are called (7 before), the two new ones (an MP3 rip at 0.023 and one at 0.080 from
  48 kHz) already `Fake` by their wall. The statistics tried before (band levels, 100 Hz holes, the
  side channel, the top octave's swing, coefficients at the 16-bit floor on AAC's grid) separated
  nothing. No Apple or FhG encode weighed (none on this machine). **Opus** lowpasses at 20 kHz at
  any bitrate and its CELT bands are not on a lattice of their own: an Opus transcode is `Suspect`.
  A direct search was run (the decode un-pre-emphasised, CELT's MDCT of 960 lines under its
  120-sample-overlap window at all 960 frame offsets, each of the 21 bands' log2 amplitude against a
  lattice of 1, 1/2 and 1/4): the strongest offset read a mean z² of 1.7 to 2.3 at 64, 128 and 192
  kbps against 1.9 for a non-Opus source and no sharp peak at any offset (neighbouring offsets read
  alike), as CELT codes a band's energy as an integer residual from a prediction on the previous
  frame's and the lower bands' running term, so the energies themselves sit on none; the residual
  wants libopus's mean table, its pitch post-filter and stereo state, and was not built. **MP2**
  (twolame) has its steps in the subband domain on a linear grid per scale factor (3 levels to 65
  535, scale factors at 2^(-k/3)), not on log steps, and is not weighed.

## Studies in the catalog

- **`track_studies`: one row per track, trigger-forgotten on a changed file.** Holds verdict +
  basis, levels, loudness, `band_levels` and `band_peaks` (above), print, recognition (`recognised`, `heard_as`, score, title, artist,
  `Agreement`). `track_studies_forget_a_changed_file` deletes the row when an update moves size,
  mtime or span (a trigger, not an upsert clause: covers every writer). Tag runs are the one writer
  known not to touch audio: `files_retagged` holds a written file's `track_studies` and `unstudied`
  rows across its follow (`KeptAcross`, `library.md`) and puts them back.
- **A failed study is kept as failed.** `unstudied` holds the id under the failing `JUDGED_UNDER`;
  `to_study` and `Library::will_not_study` skip it until `unstudied_forget_a_changed_file` fires,
  `JUDGED_UNDER` moves, or `refresh` re-asks. Stopped studies and out-of-reach files
  (`Error::is_out_of_reach`: unmounted drive, downed share) leave no mark.
- **Enrichment studies every track beside the pass.** `Studies`: `workers_for` = half
  `available_parallelism` (min 1, capped at the work asked) threads named `resonate-study-<n>`, one
  shared counter, started before the pass, joined after the pictures. `to_study` = no study, older
  `JUDGED_UNDER`, or unrecognised print (only where something can recognise; `refresh` re-asks from
  the kept print, no second decode), or a current study with no `band_levels` or no `band_peaks`,
  which is measured alone (`ToStudy::unmeasured`) and never studied again for it. The list says only *whether* a print is kept
  (`ToStudy::print_held`); the worker reads it on taking the row (`Library::print_held`), so prints
  are never all in memory. `EnrichOptions::studies` (`study` key) leaves the pool unstarted; the
  fingerprint route still studies the one track it recognises.
- **Studies read what the player reads**: `Library::sources` carries `VaultFiles` where a vault is
  held.
- **Once per track, first arrival wins** (`Claims`, shared by pool and pass): workers skip claimed
  tracks; the fingerprint route waits, then reads what landed.
- **Recognition is weighed against the row's name.** `enrich::agreement` takes matches at the
  library's `HEARD_AT_LEAST` (80): none `Unheard`; file named no title `Unnamed`; match on the row's
  recording id (own `tracks.mbid` or paired release row's) or title+credit via `same_name` and
  `same_credit` `Agrees`; else `Disagrees`. A refused lookup stamps nothing (re-asked), but
  `RECOGNITIONS_REFUSED_BEFORE_GIVING_UP` in a row (`EnrichProgress`, across workers, reset by any
  answer) ends print asking; studying continues.
- **A recognition made on another device is weighed again here.** A study landed by sync carries no
  print, so `prints_wanted` asks a connected device for it by the `WholeFile`; the print lands on a
  copy at this `JUDGED_UNDER` that holds none, and a recognition with it is re-weighed by
  `enrich::agreement` against the receiving row's own title and credit (`sync.md`), then stamped as
  recognised so `to_study` no longer asks AcoustID for it. A print with no recognition leaves one owed.
- **`Route::Fingerprint` reads the stored recognition first**, studying via the same claim if none
  is kept; a strict-score match = `Certainty::Nearly`: audio fills a name the file never gave, never
  overwrites one it did.
- **A heard name becomes the row's only when a listener says so** (a score is no reason to rename a
  tagged file). `Library::take_what_was_heard` (`enriched::land_what_was_heard`), one transaction:
  `heard_title` → title, `heard_artist` → artist (repoints `artist_id` via
  `store::artist_named_in`), `heard_as` → `tracks.mbid`, *replacing* the file's recording id (tags
  that misname a file misname its tag ids). Other claims go too (ISRC, release-track id, release
  title, numbers, `artist_mbid` where a heard artist is billed) plus any `release_tracks` pairing.
  Then rematch the album: seat the track on the release's row for that recording, or leave it
  unpaired and list the row it left missing. Stamps `answered`, clears `asked`, marks the study
  `Agrees` (rescan keeps it all, `library.md`). Analysis pane: *Take this name* where audio disagrees
  with the file or the file names nothing (`AnalysisModel::take_the_name`);
  `resonate studies --take <FILE>` is the same gesture.
- **`is:fake`, `is:suspect`, `is:misnamed` are `Shape`s**: an `EXISTS` over `track_studies`.

## Recognition

- **Seam `Fingerprints`; a `Sounded` carries the print.** Library never prints for the service,
  service never decodes. `Fingerprinters::recognise` offers each printer the print in turn until one
  matches; answers a `Recognition` (matches, whether a printer refused).
- **`AcoustId` = `resonate-online`'s printer, listener's registered key.** `Host::AcoustId` paced at
  `ACOUSTID_INTERVAL` (three a second). POST of a form to `/lookup`, gzipped (prints overflow query
  strings; `Posted::packed_form` falls back to plain). Each result's recordings → `RecordingMatch`es
  scored by the result's score; repeats keep the best, untitled dropped. `acoustid-key` is empty by
  default; `online::fingerprinters` registers `AcoustId` only once it is set and `online` is on
  (`ByEar` sits behind it, `online.md`).

## The command line and the pane

- **`resonate analyse` takes a cut or a file, read as the queue reads it**: URI via `from_uri_within`
  (`#frames=` names the same cut), `.cue` via `read_cue_media` + `--track`. Own errors:
  `SheetWithoutATrack`, `NoSuchSheetTrack`, `TrackOutsideASheet`.
- **`Pane::Analysis` follows the playing row, analyses only while in front.**
  `AnalysisModel::follow` runs from the render; a new row stops the old `Watch` and starts
  `Player::analyse` on the background executor; `ANALYSES_KEPT` rows held per run. Condensed once on
  landing (`WAVEFORM_COLUMNS`, `SPECTRUM_COLUMNS`); spectrogram painted once per row and ramp in the
  background (a frame computes nothing).
- **What the pane learns goes to the catalog.** `settled` writes the study where the catalog holds
  the row and none is kept, never over one (it would drop a landed recognition), recognising via
  `Library::recognise`. A row no scan has seen is recognised through the registry directly, no
  agreement. The recognition card leads where the audio isn't the song the file names. A refused or
  unreachable recognition is held for the row on screen only: following the row again asks again
  (`RECOGNITIONS_KEPT` keeps what was heard, never a refusal, for the run).
