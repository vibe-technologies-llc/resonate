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
  loudest of each node over its two neighbours each way (a tenth of a second, so the fit's
  `G_near` may span a fifth of a second around a delay, `eq.md`), the tiles are paired down to at most 1 024, then gathered loudest
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
  MP2 and Opus are not on log steps and are read otherwise (below). First two channels, lossless
  codecs only.
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
    control 576 off, rates up to 96 kHz. **Scale-factor band norms are read beside the lines** at
    44.1 and 48 kHz, the rates whose band layout `LONG_SCALE_FACTOR_BANDS` (49 bands) and
    `SHORT_SCALE_FACTOR_BANDS` (14 a window) hold: perceptual noise substitution fills a band with
    noise scaled to a norm of exactly its scale factor, `2^(k/4)` on the lines' own lattice, so the
    noise lines are free but the band's norm is on the steps (one faac `-q 100` master: its lines read
    0.034 over 412 000 effective, the norms of its bands 0.34 over 26 000, the genuine source 0.007).
    Each whole band inside the grid's span gives `‖band‖` in L, R, mid and side as one observation
    weighed `SCALE_FACTOR_BAND_WEIGHT` (4). A coded band's norm is `S·√Σq^(8/3)`, on the lattice at
    another phase than a q-1 line (two lines of 1 sit a whole step over one), so the norms are summed
    apart from the lines: a `Tally` carries the lines' resultant and the norms' (`Tally::banded`),
    and its lattice is `(|Σ lines| + |Σ norms|) / Σ w` (the effective count over both). Summed into
    one resultant, ffmpeg's AAC at 128 and 256 kbps lost a master whose norms and lines point apart
    (0.46 read 0.037); norms of dense bands alone (nine of ten lines heard) found 33 faac `-q 100`
    masters, all of them 36.
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
  - `Mp2` (`subbands.rs`): Layer II keeps each subband's 36 samples a frame as three *parts* of 12,
    each part's samples `sf · 2m / n` for a scale factor `sf = 2^(1 - i/3)` (`i` 0..62) and a
    class of `n` levels (3, 5, 7, 9, 15, 31 ... 65 535, `m` within ±(n-1)/2), a linear grid per
    part with a step from a table. The polyphase analysis `Mp3` runs (`granules::Polyphase`, its
    vectors kept by position for both) gives back the decoder's subband samples in its own units,
    so a part is *on the steps* where some class and one of the scale factors within an octave
    above its peak (`SCALE_FACTORS_ABOVE_THE_PEAK`) puts every sample within
    `OFF_THE_STEP_AT_MOST` (0.1) of a level, the peak at a level the class has and at least
    `SPOKEN_LEVELS_AT_LEAST` (3) samples off level zero: one spike among near-silent samples sits
    on any coarse grid (a synthetic free stream read 0.035 without that), and a part whose peak is
    under `PEAK_COUNTED_FROM` (32 sixteen-bit steps) is not counted. The lattice is the share of
    counted parts on the steps (`Tally::counted`) and `coefficients` the parts counted; there is
    no phase. 384 residues (32 polyphase offsets times 12), 0..20 kHz (subband 0 to the last coded
    one), L and R, 12 parts a search, 1 200 confirmed, control half a subband sample (16) off,
    rates up to 48 kHz (`subband_samples_on_a_scale_factors_levels_are_found_at_any_offset_and_free_ones_are_not`).
  - `Opus` (`celt.rs`): CELT codes each band's energy (log2 of the band's norm less
    `ENERGY_MEANS`, RFC 6716's table) as `α·max(E_before, -9) + Σ (1 - β)·q_lower + q + f`: a
    prediction from the same band of the frame before (α 0.5, β 0.2 between frames of 20 ms; α 0,
    β 0.15 in an intra frame), the integer coarse steps `q` of the bands below, its own `q`, and a
    fine offset `f` of `(k + 0.5) / 2^B - 0.5` for `B` fine bits (plus one finalising bit), and the
    decoder scales each unit-norm band shape to that energy exactly. So the decode is
    pre-emphasised again (`1 - 0.85 z^-1`), taken through CELT's 1 920-sample MDCT (960 lines, 120
    overlap under its power-sine window, 420 zeros each side) or its eight 120-line short blocks,
    each band's energy measured (`SCALE_OCTAVES` 1.25 between our orthonormal transform and
    CELT's), the frame before taken the same way, and the residual rebuilt band by band with each
    `q` the rounding of what is left: `f` is then a multiple of 2^-(B+1), whole turns at
    `TURNS_AN_ENERGY_STEP` (32) turns an energy step for any `B` up to 4. The phase of `32·f` is the
    lattice, each band weighed by how far its norm stands over the 16-bit rounding of its lines (a
    smoothstep 6 to 10 octaves over it). Eight kinds (the frame before long or short, this one long
    or short, inter or intra), searched on the long inter kind alone (`Grid::searched`), 960
    residues, 21 bands up to 20 kHz, L and R, 16 frames a search, 400 confirmed, control 540 off;
    48 kHz only, the rate every Opus decoder gives
    (`band_energies_on_the_coarse_and_fine_steps_of_their_prediction_are_found_and_free_ones_are_not`).
  - **The best Opus candidate is confirmed again with the pitch post-filter undone**
    (`Grid::UNDOES`, `Grid::undone`, `Pass::Undoing`; `celt::Undoing`, `pitch.rs`). A decoder runs
    a comb `y(n) = x(n) + G·Σ taps·y(n - T ± k)` over each frame's output (RFC 6716 4.3.7.1; `T`
    15..1 022, `G` = 3/32 times 1..8, three tapsets) before the de-emphasis, so a filtered frame's
    band energies are off their steps by far more than a fine step (with libopus at complexity 5
    and over, a loud master read 0.10 where complexity 4, which has no pre-filter, read 0.90). The
    re-emphasised decode is the comb's output, so `x = y - G·Σ taps·y(n - T ± k)` (FIR on what is
    held) gives back what the MDCT coded. In the output frame the previous frame's filter holds
    the first 120 samples, the two cross over the next 120 under the square of the overlap window
    (`own_share`) and the frame's own holds the rest and the next frame's first 120, so a frame's
    energies want its filter and the one before: each confirmed frame is a link in a chain
    (`Undoing::next`, broken by any other position or `forget`), the filter chosen for one frame
    the one undone over the next one's start and its energies the next one's prediction. The period
    is found as the encoder found it, from the decode: `Pitch` is libopus's pitch analysis
    (`pitch_downsample`'s halving, fourth-order whitening and added zero, `pitch_search`'s coarse
    and fine correlation and `remove_doubling`'s submultiples, continuity on the previous period
    and gain) over the 1 024 samples before the frame's own region and the 960 of it; it gave the
    period the stream carried in 61% of filtered frames and within 2 in 90% (read from libopus's
    `OPUS_GET_PITCH` on one master). Candidates are no filter and each period within
    `PERIODS_EITHER_SIDE` (2) of it under every tapset and each gain up to one step over the
    encoder's own (`0.7·` the analysis's gain, quantised as libopus quantises it), and no filter
    at all where that gain is under `ENCODER_FILTERS_FROM` (0.2, libopus's lowest threshold for
    turning its pre-filter on; without it loud white noise read a significance of 4.1); the frame takes
    the one whose energies sit strongest on the steps (`|Σ|²/Σw²`), over two histories where the
    previous frame was filtered: the chain's, and a fresh one with no filter before, so a wrong
    choice does not stay wrong (the chain alone found 0.03 on a master the oracle's periods read
    0.69 on; the fresh history put it at 0.40). The comb is linear, so each lag `T - 4..T + 4` is
    one MDCT of the shifted, crossfaded signal, a candidate's spectrum is the tapset's sum of
    them, and each gain's band powers are `‖h‖² - 2G⟨h, c⟩ + G²‖c‖²`. The choice among about a
    hundred candidates lifts chance on a genuine source (significance 3.2 at most before, 4.1 with
    the undoing, under the verdict's 12; any gain at all read 6.5),
    and the undone reading replaces the plain one only where its lattice is higher. The search stays
    plain, so `Opus` takes 16 frames a search (8 left a 64 kbps master and one decoded to 44.1 kHz
    on another offset, though undone at its own it read 0.29 over 14 000 bands).
- **A decode taken to another rate is weighed brought back.** `Weighing` runs the three grids at
  the stream's rate and feeds the first two channels through a `Resampler` (`Quality::Balanced`,
  linear phase) to each rate in `BROUGHT_BACK_TO`, running `Aac` and `Mp3` there at
  `SEARCHES_RESAMPLED` (2) places: 44.1 kHz to 48, 48 to 44.1, and 88.2, 96, 176.4 and 192 kHz to
  both CD rates (a lossy CD-rate stream played out at a hi-res rate keeps its grid however the
  upsampler's wall was hidden). A linear-phase round trip puts the grid back on whole samples and
  its passband leaves 4..20 kHz as it was: ffmpeg's AAC at 256 kbps read 0.364 decoded and 0.364
  brought back from 48 kHz. Each grid stops taking samples once done, so only the first 20 s or so
  are resampled. A `Steps` carries its `coding` and the `rate` its grid stood at; a rate other than
  the stream's is `resampled_from`. A decode downsampled below the rate it was coded at keeps its
  grid below the new Nyquist, so 16, 22.05, 24 and 32 kHz are brought back to both CD rates too:
  the lines the downsampling cut are silence there and weigh nothing, and the rest are as decoded
  (ffmpeg's AAC at 256 kbps, LAME at 192 kbps and twolame at 256 kbps taken to 32 kHz were found
  in 36 of 36 each, lattice up to 0.45 and 0.50, where none had been; the sources taken to 32 kHz
  in none). `Mp2` and `Opus` run brought back as `Aac` and `Mp3` do.
- **Taken at a lattice per coding, over its control, over enough coefficients.** The verdict takes
  a grid at `AAC_LATTICE_AT_LEAST` (0.02), `MP3_LATTICE_AT_LEAST` (0.02),
  `MP2_LATTICE_AT_LEAST` (0.02), `VORBIS_LATTICE_AT_LEAST` (0.012) or `OPUS_LATTICE_AT_LEAST`
  (0.05), `LATTICE_OVER_ITS_CONTROL_AT_LEAST` (4) times its control,
  `STEPPED_COEFFICIENTS_AT_LEAST` (20 000 effective; `STEPPED_GROUPS_AT_LEAST`, 2 000, for MP2's
  parts and Opus's band energies, each a group read whole) and `LATTICE_SIGNIFICANCE_AT_LEAST` (12) over
  `1 / √coefficients`: a periodic or synthetic signal that happens to cluster does so at every
  offset, which the control refuses, and a lattice among few coefficients is chance, which the
  significance refuses (a genuine track read 0.019 on 21 000 effective coefficients, a significance
  of 2.8; a real MP3 rip reads 0.023 on 300 000, 12.6). Every grid's `Steps` is kept in the study
  and the kept analysis (`WRITTEN_AS` 5, the peaks over moments after the print; 4: one moment; 3: a count, then coding name,
  rate, lattice, control and coefficients each, the last now the effective count); `track_studies`
  stores only the verdict (no basis column), so the change moved `JUDGED_UNDER` to 4 and every
  study is weighed again; MP2, Opus and the low rates brought back moved it to 5, the post-filter undone and the band norms to 6. Cost: about 2 s a track over the decode (3.9 s to 5.9 s for a 3:33 FLAC
  on a loaded machine, 0.9 s to 3.3 s for 40 s at 96 kHz, which runs two resamplers), fixed
  whatever its length, since every grid is done within its first 20 s; the band norms added 0.7 s
  a track to `Aac` alone (118 s to 144 s for the 36 sources, two cores at the lowest priority on a
  loaded machine) and the undoing and 16 frames a search 0.4 s to `Opus` alone (7.1 s to 22.7 s
  for the 36 sources) (`coefficients_on_the_quantiser_steps_are_found_at_any_frame_offset_and_free_ones_are_not`,
  `lines_on_the_quantiser_steps_are_found_through_the_hybrid_filterbank_at_any_offset`,
  `residues_on_vorbis_floor_are_found_on_any_short_block_grid_and_free_ones_are_not`, and in
  `tests/references.rs` the ffmpeg AAC, faac and fdkaac, dithered and hi-res, resampled AAC, LAME,
  libvorbis, twolame and ffmpeg MP2, libopus and downsampled AAC and LAME round trips beside
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
  nothing. No Apple or FhG encode weighed (none on this machine). The faac misses were its
  perceptual noise substitution, on by default (`--pns 0` found the three tried): read as band
  norms beside the lines (`Aac`, above), a second corpus of 36 (40 s from 0:40 of each, 28 at 44.1
  kHz and 8 at 48, chosen evenly through the library) went from 29 to 36 at `-q 100` and 33 to 36
  at `-q 200`, `-b 64` and `-b 160` 36 and 36, ffmpeg's AAC at 128 and 256 kbps 36 and 36; the
  sources read a significance of 3.3 at most either way (lattice 0.022 at most, on 18 000
  effective, under the count the verdict asks). **MP2**
  (`subbands.rs`, above): twolame at 128, 192, 256 and 384 kbps and ffmpeg's MP2 encoder at 256
  kbps 36 of 36 each (0.84 to 1.0), twolame at 256 kbps dithered (triangular), taken to 96 kHz and
  taken to 32 kHz 36 of 36 each; the sources read 0.0013 at most. **Opus** lowpasses at 20 kHz at
  any bitrate, so its wall alone said `Suspect`; its band energies on their predicted steps
  (`celt.rs`, above) find libopus at 64 kbps in 31 of 36, at 128 and 192 kbps in 30 (lattice up to
  0.92), at 128 kbps decoded to 44.1 kHz and brought back in 28. With the post-filter undone and 16
  frames a search, the second corpus went from 32 to 35 at 64 kbps, 34 to 35 at 128 and 192 kbps
  and 33 to 36 decoded to 44.1 kHz (the median significance of a found master from about 50 to
  80; counted before `ENCODER_FILTERS_FROM` was added, not counted again since); the one left is a quiet choral master with too few bands over the rounding (2 000 where a
  loud one has 16 000), and the sources read a significance of 4.1 at most (3.2 plain). The
  sources read 0.032 at most on 8 800 bands (a significance of 3). An earlier direct search of
  the energies against a lattice of their own found nothing, as the prediction carries every
  frame's energy off any fixed grid. The whole library (462 tracks, their first 45 s) calls the
  same 9 tracks as before, all on AAC's or MP3's steps; MP2 reads a significance of 0.64 at most
  and Opus 3.8 at most (on a track already called from AAC).

## Studies in the catalog

- **`track_studies`: one row per track, trigger-forgotten on a changed file.** Holds verdict +
  basis, levels, loudness, `band_levels` and `band_peaks` (above), print, recognition (`recognised`, `heard_as`, score, title, artist,
  `Agreement`). `track_studies_forget_a_changed_file` deletes the row when an update moves size,
  mtime or span (a trigger, not an upsert clause: covers every writer). Tag runs are the one writer
  known not to touch audio: `files_retagged` holds a written file's `track_studies` and `unstudied`
  rows across its follow (`KeptAcross`, `library.md`) and puts them back.
- **A failed study is kept as failed**, a band measurement of a study kept without its bands included (`measured_now`), so a file that no longer decodes is not decoded again by every pass (`bands_that_cannot_be_measured_are_kept_as_failed_and_not_decoded_again_on_every_pass`). `unstudied` holds the id under the failing `JUDGED_UNDER`;
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
