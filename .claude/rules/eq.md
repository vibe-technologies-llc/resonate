---
paths:
  - "crates/resonate-eq/**/*.rs"
  - "crates/resonate-eq/targets/*.csv"
  - "crates/resonate-analysis/src/headroom.rs"
  - "crates/resonate-analysis/src/applied.rs"
  - "crates/resonate-analysis/src/bands.rs"
  - "crates/resonate-analysis/src/tones.rs"
  - "crates/resonate-core/src/eq.rs"
  - "crates/resonate-dsp/src/eq.rs"
  - "crates/resonate-dsp/src/convolve.rs"
  - "crates/resonate-dsp/src/crossfeed.rs"
  - "crates/resonate-engine/src/impulse.rs"
  - "crates/resonate-codec/src/sofa.rs"
  - "crates/resonate-codec/src/netcdf.rs"
  - "crates/resonate-online/src/autoeq.rs"
  - "crates/resonate-dsp/src/sweep.rs"
  - "crates/resonate-listen/src/listener.rs"
  - "crates/resonate/src/measuring.rs"
  - "crates/resonate-core/src/abx.rs"
  - "crates/resonate-ui/src/views/settings/abx.rs"
  - "crates/resonate/src/abx.rs"
  - "crates/resonate-mobile/src/abx.rs"
  - "crates/resonate-engine/src/audition.rs"
  - "crates/resonate-engine/src/engine/arms.rs"
  - "ios/Sources/Resonate/ListeningTest.swift"
  - "ios/Sources/ResonateLogic/ListeningTestWords.swift"
  - "android/app/src/main/java/org/resonate/android/ui/equaliser/ListeningTest*.kt"
  - "crates/resonate-ui/src/equaliser.rs"
  - "crates/resonate-ui/src/views/settings/equaliser.rs"
  - "crates/resonate-ui/src/views/settings/curve.rs"
  - "crates/resonate/src/equaliser.rs"
  - "crates/resonate-mobile/src/equaliser.rs"
  - "crates/resonate-mobile/src/measurement.rs"
  - "crates/resonate-mobile/src/room.rs"
  - "ios/Sources/Resonate/RoomCorrection.swift"
  - "ios/Sources/Resonate/MeasuredCorrection.swift"
  - "ios/Sources/ResonateLogic/Measurement.swift"
  - "android/app/src/main/java/org/resonate/android/RoomCorrectionState.kt"
  - "android/app/src/main/java/org/resonate/android/ui/equaliser/Measurement*.kt"
  - "ios/Sources/Resonate/EqualiserGraph.swift"
---

# The equaliser

Parametric, arbitrary bands, bound per device, AutoEq behind it. Chain: `audio.md`; stage contract:
`realtime.md`.

## The vocabulary

- **Arithmetic is `resonate-core::eq`**: stage, `explain`, `resonate eq` and the pane curve cannot
  disagree on a band; `resonate-dsp` stays a core leaf. `resonate-eq` is I/O only (EqualizerAPO and
  GraphicEQ readers, profile store, AutoEq catalogue, the `Corrections` seam `resonate-online`
  fills, the fit of a listener's own measurement).
- **`Band` fields are quantised integer newtypes**: `Frequency` centihertz, `BandGain`/`Preamp`
  milli-dB, `Q` milli-units. `OutputSettings` derives `Eq` and `publish_settings` compares it every
  16 ms; floats would not build.
- **Channels**: `Band::channels` is a `ChannelSet` (`EVERY` default) of *speaker slots* in
  EqualizerAPO's order (`L R C SUB RL RR SL SR`), not stream indices; `design_for` gives
  `Biquad::IDENTITY` off-set. The stage maps stream channels to slots through the layout
  (`ChannelSet::slot_of`), so `C` on quad reaches nothing
  (`a_channel_a_profile_names_is_the_speaker_of_that_name_whatever_the_layout`). The APO reader
  follows `Channel:` lines, the writer emits one where sets differ; an unnameable channel or a
  partial-channel `Preamp:` is passed over.
- **Curves are per channel; channel-less = loudest.** `Profile::magnitude_db_on`/`response_on`: one
  channel; `magnitude_db`/`response`/`peak_db`: loudest per point (L+R boosts at one centre = 6 dB,
  not 12), so *Fit the preamp* holds the loudest under full scale. `channels_apart`: channels some
  band reaches alone, each drawn by `Profile::responses` (`Traced` per `TracedOn`), cached per
  revision and rate in `EqualiserModel::drawn`.
- **`Band::new` normalises**: gainless kinds carry no gain, else equal notches compare unequal and
  republish `OutputSettings`.
- **`w0` clamps to π** (else a centre past Nyquist wraps `cos` and mirrors down); at π every kind is
  identity. `Band::design` is identity where `Band::applies_at` is false; `explain` prints how many
  bands a rate passed over.

## The stage

- **DF1, f64 state; both load-bearing.** DF1 words keep their meaning across a coefficient change,
  so a retune under a running stream has no transient (TDF2 residues click); coefficients swap
  between blocks, no crossfade. f64: a 20 Hz shelf at 192 kHz has poles within 3e-4 of the unit
  circle; f32 leaves a drift floor near -60 dBFS. Widened once in, narrowed once out.
- **`usable` guards the recursive path**: IIR tails decay into denormals (`unsafe_code = "forbid"`
  rules out FTZ), a NaN never leaves an IIR. Runs on the sample entering the cascade and each
  section's output.
- **Section-major over a widened block, `SECTIONS_STAGGERED` sections in flight**: `process` widens
  a piece into `Widened` (sized in `prepare`), runs each section over it, narrows once; frame-major
  cost a frame the whole cascade's latency. Bit-identical to frame-major
  (`the_stage_is_bit_identical_to_a_frame_major_cascade`). Multiply-adds fuse only where the build
  targets FMA (`fused::multiply_add`; `f64::mul_add` on baseline `x86-64` is a libm call).
- **Channel groups** (1, 2, 4, 6, 8 const generics; others split into eights then the rest): history
  is `Lanes`, each section reaches its frame through its own `get_mut`, so the SLP vectoriser packs
  no permute onto the feedback path.
- **Transparency is structural, one predicate for stage and plan**: `Profile::is_transparent` = no
  bands and unity preamp (ten bands at 0 dB is *not*). `ChainBuilder::build` drops a transparent
  stage; a numerical rule would rebuild the chain twice per drag as a slider crosses 0 dB.
- **A retune that moves more than one band replaces the filter set and starts its history over**
  (`bands_apart` against `BANDS_A_RETUNE_MOVES_AT_MOST`): a drag, a typed cell, an added band move
  one and carry history; selecting another profile does not, else the old filters ring on in the
  new ones. `state.fill` allocates nothing
  (`a_profile_selected_in_place_of_another_leaves_the_old_filters_ringing_nowhere`). A profile of one
  band swapped for another of one band is indistinguishable from a drag and carries.
- **`MAX_BANDS` (32) of state in `prepare`**, `[group][section][channel]`: adding a band allocates
  nothing; lanes a shrink vacates are silenced (else a thump on regrowth).
- **`Equaliser::prepare` refuses a spec off its build rate** (`InputRateMismatch`): a stage pushed
  above the resampler fails to build rather than designing at the source rate.

## Where it runs

- **After remix and resampler, before gain; dither last.** A biquad warps with its design rate, so
  it is designed at the sink rate. Before gain: the volume slider is the listener's last word and
  should attenuate a boost.
- **In force it makes the plan `Converted`.** **A DoP-packed stream refuses it**: `untouched` writes
  `equalisation: None`, `dop_survives` gates on `eq_config`.
- **A change on a stream already playing is spliced into what the ring holds** (`audio.md`): a
  fresh back from the new plan is run again over the frames the front handed on and blended in
  over `SPLICE_BLENDED_OVER`, so a change is heard a few pulls after it is asked for whatever the
  buffer. The fresh stage starts settled wet; its own easing and the retune below are the
  fallback where no splice can be made.
- **First band added / last removed reshape the chain, not reopen the stream.** `retune` swaps in a
  chain from the new plan wherever `OutputPlan::becomes_on_the_same_stream` allows (`audio.md`),
  carrying a running resampler or convolver. The stage crossfades itself: *entering* a playing
  stream it starts dry, blending to wet over `EASED_OVER` (40 ms); *leaving* blends to dry, the
  wanted plan held in `Output::settles_into`; re-asked while leaving it returns from where the blend
  stands. Settled never blends (bit for bit); a paused transport swaps at once
  (`switching_the_equaliser_on_and_off_mid_track_glides_rather_than_steps`).
- **A default build is flat and off**
  (`an_equaliser_that_is_switched_off_leaves_every_plan_the_shape_it_had`); enabled over an empty
  profile is bit-perfect too (`eq_config` filters on `is_transparent`).
- **Plan and chain judge transparency separately, held to one answer**
  (`a_plan_that_says_it_touches_the_signal_builds_a_chain_that_does`): a disagreement once put raw
  f32 bytes at the sink's integer stride. `Engine::fill`/`flush` branch on the plan.
- **The preamp is a scalar before the bands.** Nothing else guards a boost: clip prevention
  attenuates, never limits (`audio.md`). *Fit* sets it from `peak_db`, which weighs every band's
  centre beside the drawn sweep (a Q 40 boost between two points is not missed); a peak past the
  deepest preamp fits the deepest. `Preamp::holding_under_full_scale(peak_db)` is the one rule from a
  peak to a preamp (nothing at or under 0 dB, the deepest past it), shared by the model and the
  measured fit below. **It glides**: `set_equalisation` moves `Preamping` over
  `EASED_OVER`, weighed by frames since it began (lands exactly); `is_ramping` meanwhile
  (`a_preamp_change_is_eased_in_rather_than_stepped_within_a_sample`).

## Fit the preamp to the music

- **The fit covers everything a sample meets after the preamp: the room correction, then the
  bands.** `resonate_analysis::Applied` is the one input of every fit: a `Profile` and the
  `Impulse` of the room correction in force (`Applied::from(profile)` alone, `corrected_by(Some(impulse))`
  with one), so the model, the decode and the bound read one chain and cannot disagree on it. Its
  responses compose the correction into the profile's: `Applied::heard_at` multiplies the
  correction's complex response (`Impulse::heard_at`, the taps summed turned by each frequency, the
  same turn as `Profile::heard_at`, at the stream rate through `Impulse::at`) into every channel
  response of the profile, one product for each profile channel group and each channel of the
  correction, since the fit does not know which stream channel a speaker slot meets (the loudest
  pair counts); `Applied::impulse_responses` filters each correction channel's first taps through
  the profile's biquads (`Profile::filtered`, of which `impulse_responses` is the unit-impulse case),
  which is the composed response, exact for the frames it holds; `Applied::peak_db` and
  `fitted_preamp` are the model's sine over that product. **The volume stage is left out, by
  reason**: `Volume` is 0 to 1 and its gain its cube (`Volume::to_gain`), so the slider can only
  attenuate, and it sits after the equaliser, where a boost is meant to be attenuated by it
  (*Where it runs*); a ReplayGain boost is per track, from tags the fit does not read, and is capped
  by the source's declared peak with the true-peak guard behind it (`audio.md`). A preamp that holds
  the stage under full scale holds the gain stage under it at every position of the slider.
- **The model fits a sine; the music is measured.** `Profile::fitted_preamp` assumes full scale at
  the curve's loudest point, so a bass boost over a song with a quiet low end costs headroom it
  never uses. `resonate_analysis::peak_through(sources, location, span, applied, watch)` decodes the
  cut as a study does (`Walking`, the shape it opened in), runs the real `Convolver` (where there is
  a correction, prepared at the file's rate) and then the real `Equaliser` stage over it
  at the file's rate, on the profile `at_rate` that rate and with its preamp taken off, and returns
  the output's true peak (`TruePeakMeter::at`, the guard's own interpolator); the convolver's tail is
  flushed through the stage once the song ends, so a response that holds the music back past its end
  is measured too
  (`a_response_holding_the_music_back_past_its_end_is_measured_in_the_tail_it_flushes`). The preamp is a
  scalar before the correction and the bands, so one decode answers: `Preamp::holding_under_full_scale` of that peak.
  A correction that boosts where the music sits fits deeper than the profile alone and one that cuts
  there fits shallower
  (`a_room_correction_that_boosts_where_the_music_sits_makes_the_fit_deeper_than_the_profile_alone`,
  `a_room_correction_that_cuts_where_the_music_sits_needs_less_preamp_than_the_profile_alone`).
  Music at full scale on the boost fits as the model does
  (`music_peaking_at_full_scale_on_the_boost_is_fitted_as_the_curve_alone_is`); a boost where the
  music has no energy fits less (`a_boost_where_the_music_has_no_energy_needs_less_preamp_than_the_curve_alone_says`);
  intersample peaks and a sum of components can fit more than the model.
- **A study keeps what a fit needs, so the desktop fits to the whole catalog.** Every study
  stores `track_studies.band_levels` (`BandLevels`: 29 third-octave bands from 25 Hz, the first
  taking everything under 31.5 Hz and the last everything over 16.1 kHz, each one byte, half-dB
  steps below the loudest band; 255 is a band the music never sounded in, all 255 a silent song)
  and `track_studies.band_peaks` (`BandPeaks`: up to `MOMENTS_KEPT` (8) moments, each the peaks of
  the 57 nodes of the band tree over a stretch of the song, `analysis.md`), both measured in the
  study's one decode. The fit reads only the peaks. **The bound** (`BandGains::of(&applied, rate)?`,
  then `BandGains::bound(peaks)`; fallible because the correction is redrawn at the fit rate, which
  can refuse a response too large to hold, `Error::Correcting`): the output is `c * x + sum over S of (G - c) * x_S` for any level
  `c` and any partition of the bands into tree nodes `S`, since the bank's bands sum to the signal
  exactly; each term is bounded by the node's peak times its *reach*, and the tree is searched
  bottom up for the cheapest partition (`cheapest`), so broadband music pays one peak for a span
  the curve moves alike instead of the peaks of every band in it as if they fell together. A node's
  reach at level `c` is `max over f of share_S(f) * |G_near(f) - c|`, `G` the complex response
  (`Applied::heard_at`, phase counts; per-channel bands count on the channel they reach furthest,
  the correction's response multiplied in)
  on 2 048 points from 1 Hz to Nyquist plus every band's centre. `share_S(f)` is the bank's own
  leakage, not a flat widening: with `B_S(f)` the node's real response (`split::passed_below`, the
  band-edge filters at the fit rate) and `phi(B) = min(1, B / 0.25) * B`, `share_S = min(1, B_S /
  0.25) / (phi(B_S) + sum of phi(B_b) over the bands b outside S)`; since `phi` is superadditive,
  the shares of any partition cover every frequency at least once (`sum of share_S * B_S >= 1`),
  so a steady tone anywhere is bounded at its own response, and a node pays for a frequency in its
  skirt only by the share of it the node holds. The levels are 0, 1 and eight geometric steps from
  the curve's quietest gain (at least 0.25) to its loudest; each moment takes its least level and
  the bound is the loudest moment's. **The solved bound** (`tones.rs`): the partition is one
  feasible answer of the tone model's dual, so each moment and level is also solved exactly as a
  linear programme, `min sum lambda_S * P_S` subject to `sum lambda_S * B_S(span) >= D(span)` over
  346 spans (24 an octave from 1 Hz to Nyquist, each span's `B_S` the least of its two edges, a node's
  response being unimodal, `D` the most `|G_near - c|` reaches inside it), by a revised simplex from
  the slack basis (`Solving`, warm across levels, 57 rows); any dual it stops at is made feasible
  by scaling and so is an answer, it stops within `SETTLED_WITHIN` (0.1 %) of the primal, gives up
  once the primal passes what a level already holds, and a level whose loudest single tone
  (`heard_alone`) cannot win is never solved; the moment takes the least of the partition and the
  solution. This lets a quiet band carry a boost it borders instead of the loud band whose skirt
  reaches it, and a mixed weighting of nodes no partition offers. **`D` is an envelope, not a
  sample**: the near response is transformed on `ENVELOPE_BINS_A_TAP` (32) bins a tap and each
  bin pays `|Z - c| + (pi/N)|Z'| + (pi/N)^2/2 * sum (tau - e)^2 |h|` (its value, slope and a bound on
  its curvature), so a comb's teeth between points (an echo inside the window) are paid in full.
  **The whole signal's node is priced at its own transient gain**: with `r` the near window less `c`
  at its main lobe (`Soon::swung_by`, the most of any channel), the term `r * x` peaks at most at
  `L1(r) * P_W` whatever the music (Young's inequality), so the partition's root reaches at least
  `L1(r)` and the programme holds the whole node with its price multiplied by `L1(r)` over the most
  `D` reaches (`Anchor::transient`, never under 1): alone it costs that rigorous term, and it joins
  the other nodes no cheaper. At its steady price the root passed under the decode of a curve that
  only turns phase, claiming at level 0 that `|G| = 1` could never raise a peak: an allpass pair
  alone (300 Hz and 3 kHz, Q 4) bounded noise 0.91 dB and a kick, hat and pads 0.81 dB under their
  decodes (`a phase turned and nothing else` in the test below); in the programme at its steady
  price it passed the decode of a turned curve by up to 0.38 dB, which is why it was once left out.
  Every other node keeps its steady price: weighing each by its band's transient gain too (the L1
  of the near response filtered by the node's band, over the band's own, over its steady gain) was
  measured and dropped, since a dispersive correction's band responses have an L1 far over their
  steady gain (a dispersive burst bounded 6 to 12 dB looser) and it moved the mean of the corpus
  below from 2.36 to 2.89 dB over the decode while the root alone holds every case. **A node that held nothing
  covers nothing** (`Solving::covering`): it is left out of the programme (its room, its prices,
  its columns), never offered as a free coverer. Every band's filter leaks into its neighbours, so
  a node reads nothing beside one that held something only in figures no decode makes
  (`BandPeaks::at_once`); with it in the programme a lone band at 0.9 under a +6 dB bell bounded
  at 0 where the decode is 1.8, since the empty neighbours covered every span at no cost and the
  solution passed under the truth. A span no remaining node reaches leaves the programme without
  an answer and the partition stands
  (`a_lone_band_under_a_boost_on_it_is_bounded_at_least_at_its_boosted_peak`). No moment of the
  decodes the bound is measured against held an empty node beside a sounding one, so the figures
  below are unchanged. **Time**: a moment holds every
  node's peak over its stretch widened by a tenth of a second each way, so the response may be
  split around a delay `d`: the chain's impulse response (`Applied::impulse_responses`, the
  correction's taps run through the profile's biquads, the whole correction long) is searched for
  its loudest tap `e`, and a window of a fifth of a second holding `e` outside its tapered quarters
  is placed where it holds the most energy (`Soon::within`; it starts at 0 for a profile alone);
  that window is `G_near`, measured against `c * e^(-i w e)` (the level delayed to the main lobe),
  and both meet the moment that holds `t - d` for `d` the window's middle. The rest (`G - G_near`) is
  bounded apart (`ringing`, at level 0, with the points' fixed shares) by the loudest of each node
  over all the moments, and **between the points by an envelope, not a sample** (`ringing_of`): the
  chain's response is run on past the correction until its last quarter of settling holds under a
  millionth of its L1 (`Rung::of`, settling doubled from the window's length, at most
  `SETTLING_SECONDS_AT_MOST` (8 s)), the window's taps are taken out of it, and what lies beyond is
  paid as the geometric remainder of its last two blocks, as are the ends of the ring holding under
  a millionth of its L1 (`trimmed`, so a long quiet tail does not lengthen it). The ring's taps,
  and the same taps times their distance from its middle and that distance squared, are transformed
  on at least `RING_BINS_A_TAP` (8) bins a tap (at most 2^21, never under twice the taps), and each
  point pays the most any bin cell between its neighbours' midpoints reaches (`enveloped_between`).
  A cell pays the lesser of two rigorous bounds, with `n` the ring's half-length, `N` the bins and
  `S` the loudest bin over `cos(pi n/N)` (the true loudest is at most that, by Bernstein and Szegő):
  the larger of its two bins plus linear interpolation's error `(pi n/N)^2 / 2 * S`, or the larger
  of its two ends' value, slope and bend (`|Z| + (pi/N)|Z'| + (pi/N)^2/2 |Z''|`) plus the
  third-order remainder `(pi n/N)^3 / 6 * S`, so a ring loud at one frequency adds little where it
  is quiet; never under the sampled response. A long reverberant tail's teeth fall between the
  2 048 points: two seconds of noise a quarter of a second late under a direct tap bounded a tone on
  a tooth up to 0.69 dB under its decode, now over it
  (`a_tone_on_a_tooth_of_a_long_reverberant_tail_is_bounded_where_the_tail_meets_it`); over the
  corpus below it moved the mean from 2.35 to 2.38 dB over the decode (an echo or a comb past the
  window 0.14 dB looser on average, at most 0.53). A linear-phase correction's main lobe, half its length late, is
  therefore paid moment by moment
  (`a_correction_whose_energy_comes_late_is_bounded_where_its_main_lobe_meets_the_music`,
  `music_heard_apart_through_a_late_linear_phase_lift_is_bounded_moment_by_moment`), and a causal
  curve's ring is paid within twice the span it was. `Applied::impulse_responses` orders its
  responses as `heard_at` does (profile group outer, correction channel inner), so near and whole
  are subtracted channel for channel. **What is rigorous and what is not**: the sum over a
  partition is exact, the moments hold every stretch with its neighbours, the codes round up, the
  near envelope holds between bins, the ring is bounded by the whole song and its envelope between
  the points, the whole node alone
  pays its L1, but `|G - c| * peak` bounds a narrower node's term exactly only for a steady tone or
  a sustained sum of tones; a transient
  through a resonant or phase-turning curve can pass it by up to the filter's L1-to-peak ratio, the
  band peaks are read at each sample and its cubic midpoint scaled by a tone's shortfall at the
  band's reach, the bank is zero-phase only to within its margin (`analysis.md`), and the bank's
  response is taken at the fit rate though the study ran at the file's. **Measured** over eight
  songs (a tone low and high, bursts, beats, noise, a band, clicks, bass then treble) under
  thirteen curves with no correction and four curves under each of nine corrections (the six
  below, a 32 767-tap linear-phase low pass, the same length of linear-phase bass lift, and that
  lift padded to ten seconds), 375 cases with the decode above -20 dBFS: the bound sat 3.21 dB over
  the decode on average before, 2.87 with the window split around the main lobe and 2.02 solved
  (1.66, 1.64 and 0.98 with no correction; 3.49, 1.95 and 1.25 under the bass lift), never under
  it (0.03 dB at least). With the whole node at its transient price, over the same eight songs
  under fifteen curves alone (the thirteen, the allpass pair alone and no curve) and five of them
  under each of nine corrections (the seven below, an echo half a second late and a comb a second
  long), 469 cases above -20 dBFS: 2.36 dB over the decode on average before and 2.35 after, 0.82
  and 0.95 with no correction (the allpass pair alone from 0.91 under to 3.03 over; curves the
  root's steady price had served bound up to 1.2 dB looser), 2.21 and 2.16 under a correction (up
  to 1.6 dB tighter where the whole node joins the solution), 0.001 dB over at least. The bound holds over the decode across eight songs and fourteen curves
  (`the_bound_holds_over_what_the_decode_measures_whatever_the_curve_and_where_the_music_sounds`)
  and under seven corrections (a boosting pair of taps, a lowpassing one, an echo, a dispersive
  burst, a response held back 0.3 s, a short linear-phase low pass and a linear-phase bass lift a
  third of a second long) beside four curves and six songs
  (`the_bound_still_holds_over_what_the_decode_measures_with_a_correction_in_the_chain`), and fits
  as the boost takes it where a song's peaks gather in a boosted band
  (`a_song_whose_peaks_gather_in_a_boosted_band_passes_the_estimate_and_not_the_bound`). **What
  stays loose**: broadband music under a curve that moves many bands still bounds one to three and
  a half decibels over its decode, since every node's term is summed apart
  (`broadband_music_under_a_curve_moving_every_octave_is_bounded_within_a_few_decibels_of_its_decode`);
  a tone beside a narrow boost is bounded within half a decibel
  (`a_tone_beside_a_narrow_boost_is_bounded_near_its_decode`). **Cost**: `BandGains::of` takes
  about 15 ms for a profile and 75 ms with a ten-second response (it was 28 and 320 ms here), since
  the near window is summed by `resonate_dsp::response_of`, which shares the points among
  `available_parallelism` scoped threads once taps times points pass `TURNS_WORTH_A_THREAD` (2^24),
  as `Impulse::heard_at` does for the whole response; the ring's envelope adds three transforms of
  the settled ring, one after another, about 140 ms under a ten-second room response on a loaded
  machine (the bench's `ten-second room`, about 250 ms before and 350 after there) and a
  millisecond under a profile alone, and holds at most 2^21 complex bins (32 MiB) at a time unless
  the ring is longer than half of that. A
  solved bound costs about 0.36 ms a song and a fold about 50 us a song (`cargo bench -p resonate-analysis --bench bound`: twelve songs
  under six curves, a thousand songs folded; 1.2 ms and 156 us before) where the partition costs
  microseconds: the simplex prices the spans against each node's shares laid node by node over the
  contiguous spans it passes (`Tones::reaches`), so the sums run across spans and vectorise, it
  passes over every node whose dual weight is exactly zero (four in five of them), and the entering
  column is summed with its rows interleaved. Each is the same additions in the same order, so
  every bound is bit for bit what it was (`RESONATE_BENCH_BOUNDS=<file>` writes the bench's bounds
  to compare). The whole node's honest price leaves the partition looser, so more moments are
  solved (a bound about 0.5 ms a song on a loaded machine), and the fold proves sooner that a
  moment cannot win: `Loudest::weigh` calls `bound_beside(peaks, loudest_yet)`, which solves no
  moment whose partition cannot raise what the fold already holds, passes over one that the dual
  weights of any of the last `HINTS_KEPT` (4) solved moments (`Hints`, kept beside the gains,
  `Tones::paid_by`) already bound under it, and stops a level as soon as it pays under it
  (`Solving::bound`'s `enough_at`). Each of those answers is a feasible answer of the programme,
  so the fold is never under the programme's optimum, though it may stop under the loudest
  `bound` by the solver's 0.1 %; the bench's folds equal their loudest bound, and a folded song
  costs about 15 us (50 before the whole node came in, 156 before any of this). `bound` alone
  uses no hint and is the same whatever was folded before. The old
  estimate, the true peak times the mean gain weighted by each band's energy, is gone: a loud
  sustained band hid the quiet band where the peak gathered. `Loudest::weigh` takes the peaks.
- **The set is the catalog's kept bands plus the queue's unmeasured songs, bounded.**
  `Library::loudest_kept(profile, rate)` folds every kept measure in one read (`Loudest::measured`
  counts them); `Library::unmeasured(cuts)` is the cuts no kept measure of the right length
  stands for (a row of another layout, an unscanned file, any non-local location);
  `engine::cuts_to_fit(rows, playing)` picks the queue's cuts, from the playing row on, wrapping,
  each location and span once, at most `CUTS_AT_MOST` (32), and
  `loudest_through(sources, cuts, profile, watch)` measures what is left by decoding, on
  `available_parallelism / 2` scoped threads sharing one counter (`Loudest`: `peak`, `measured`,
  `passed_over`; a cut that will not decode is counted and logged, `Stopped` ends the lot), joined
  with `Loudest::joined`. `Loudest::fitted_preamp` is `None` where nothing was measured or all of
  it was silent. `Loudest::weigh` is the one fold of a kept measure into the run (the library's and
  the phones' catalog alike). `Player::cuts_to_fit` and `Player::loudest_among` serve the window;
  none of it runs on the audio thread. The run is the whole chain after the preamp (above):
  `Impulse::with_headroom` holds a read response's loudest gain to unity, but its impulse response
  can still lengthen a peak (a dispersive one sums its taps' magnitudes), which is why the fit runs
  and bounds it rather than assuming it harmless. The decode (`peak_through`) is exact for the
  chain it runs; the kept figures bound it. `Library::loudest_kept(&BandGains)` takes the gains the
  caller made, so the library never sees a correction.
- **A study kept before the bands were measured is measured alone, never studied again.**
  `to_study` lists a current study whose `band_levels` or `band_peaks` is missing or of another
  length as `unmeasured` (the thirteenth step, `PEAKS_KEPT_AS_ONE_MOMENT_ARE_MEASURED_AGAIN`, clears
  the 58-byte peaks of the layout that kept one moment, and the phones' seventeenth does the same); the pool decodes it once through `resonate_analysis::band_figures` (the
  study's own transform and bank, so the same figures) and updates those two columns, leaving the
  print, the recognition, the verdict and the `studied` stamp where they stood
  (`a_study_kept_without_its_bands_is_measured_alone_and_keeps_everything_else_it_held`,
  `a_study_kept_without_its_peaks_is_measured_alone_and_is_not_fitted_from_until_it_is`).
  A study under an older `JUDGED_UNDER` is studied whole as ever. Until a song is measured the
  fit decodes it if it is queued and passes it over otherwise.
- **The press keeps the model as the first answer.** The desktop's *Fit* sets the model fit of the profile at
  once, then `RootView::fit_the_preamp_to_the_queue` reads the room response again from the file the
  settings name (`read_impulse`; one that will not read is logged and left out), builds the
  `Applied`, and measures on the background executor under a
  `Fitting` (a `Watch` stopped when it is dropped, so a second press or the window going replaces
  it; an empty queue still reads the catalog) and `EqualiserModel::fit_the_preamp_to_the_music`
  lands the measured preamp only if the shown bands are the ones measured, toasting *The preamp is
  fitted to the N songs measured*; where nothing was measured and a correction is applied it lands
  the model fit through the correction (`Applied::fitted_preamp`) instead, toasting *The preamp is
  fitted to the room correction*. The phones read the same bands from their own catalog:
  `MobilePlayer::fitted_preamp_to_the_queue(curve)` folds `Catalog::loudest_kept` (every visible
  document whose `studies.band_peaks` is `BAND_PEAKS_BYTES` (472) long, through `BandGains::of` the
  curve and the room correction applied at the stream rate, 48 kHz with none open) with a decode of `Catalog::unmeasured(cuts_to_fit)` alone, the queue's
  songs with no kept peaks (a cut, which no study covers, a document not indexed, one studied
  before `band_peaks` existed or changed since: `documents_forget_where_the_peaks_gathered`), and
  answers the preamp in dB (`None` for nothing measured or all of it silent, held to the phone's
  `PREAMP_AT_MOST_DB`), stopping the fit before it through `Services::fitting`; an empty queue
  still reads the catalog. The pass and the analysis pane keep the bands with the study
  (`keep_studied`, `keep_study`). **A study kept before the bands is measured alone, as on the
  desktop**: `enrichment::measure_the_unmeasured` runs first in the pass's study thread, key or none
  (it asks no service and decodes only studies already kept), over `Catalog::studies_without_bands`
  (every visible document whose study lacks the 29-byte `band_levels` or the 472-byte `band_peaks`),
  decoding each once through `resonate_analysis::band_figures` and writing those two columns
  through `Catalog::keep_bands`, which leaves the verdict, true peak, print and agreement where they
  stood and never overwrites figures already whole; a stopped pass ends it, a document that will not open is left owed for the next pass
  (and decoded on a press as before). The next fit reads it without a decode
  (`a_study_kept_without_its_bands_is_measured_by_a_pass_with_no_key_and_keeps_all_else`,
  `a_song_measured_by_the_pass_is_no_longer_owed_a_decode_when_the_preamp_is_fitted`,
  `bands_already_kept_are_not_measured_again_and_a_song_without_a_study_gains_none`,
  `a_study_kept_with_its_levels_but_not_its_peaks_is_measured_for_both`,
  `a_pass_told_to_stop_measures_nothing_and_a_file_that_will_not_open_stays_owed`). Android and iOS set the model fit, then the measured one if the
  curve is still the one fitted
  (`a_queue_of_silence_fits_nothing_and_an_empty_one_or_a_bad_curve_is_told_apart`,
  `songs_with_kept_bands_are_fitted_without_a_decode_and_the_whole_catalog_counts`,
  `kept_bands_are_forgotten_with_a_changed_file_and_a_cut_row_is_never_taken_as_measured`).

## The listening test

A level-matched ABX of the equaliser against a bypass of itself, or of lossy restoration, the
resampler or dither against another setting, in the Equaliser category's *Listening test* group
(`Group::Abx`, `views/settings/abx.rs`); the arithmetic is `resonate-core::abx`, windowless and
tested.

- **A is the chain as set, B its bypass, each lowered and neither raised.** `Arms::matched(profile,
  rate)` measures `mean_gain_db` (the mean power of `Profile::response`, preamp included, over the
  log-spaced sweep from 20 Hz to 16 kHz, so equal weight per octave) and lowers whichever arm is
  louder by the difference through its preamp (A's own, or a flat B's), so no match can clip. The
  residual is measured again on the two arms as built (the preamp is quantised to milli-dB) and
  must be within `LEVELS_MATCHED_WITHIN_DB` (0.1), else `Error::LevelsNotMatched` (also where the
  preamp would pass its range) and the test does not begin. The room correction is not an arm; it
  stays on in both and is not in the match.
- **X is drawn up front from a seed**: `Session::new(planned, seed)` fixes every X with a
  splitmix64 stream (the window seeds it from the clock; 1 to `TRIALS_AT_MOST` (100) trials, the
  pane offers 8, 16 and 32), `answer(arm)` keeps a `Trial` (X and the answer) per trial and refuses
  one past the last (`Error::AbxFinished`); `Score::p_value` is the exact one-sided binomial tail
  at p = 1/2 (`binomial_tail`, summed in log space) and `is_significant` is under `SIGNIFICANT_AT`
  (0.05).
- **The switch is the equaliser's own command.** Hearing an arm sends `Command::SetEqualisation`
  of `Equalisation::auditioning(sink, profile)` (the profile bound to the sink in use and as the
  fallback), so it is spliced into what the ring holds and blended over `SPLICE_BLENDED_OVER`
  (*Where it runs*): no gap and no click, and heard a few pulls after the press, within the buffer.
  Nothing is added to the audio callback. Ending the test, or any edit of the curve, sends the
  real `Equalisation` again (`tell_the_engine`). The test needs a curve bound to the device in use
  (a toast says so), uses the shown profile `at_rate` the stream rate, and its state
  (`RootView::audition`) lives for the run only.
- **The pane shows what the listener needs and no more**: the trial and score with the chance of
  that by guessing, a line of verdicts per trial, how the levels were matched, and which arm is
  playing; an X is heard but never named, and the verdict of the last answer is shown after it.
- **The window chooses what is tested before it begins** (`Testing`: *Equaliser*, *Restoration*,
  *Resampler*, *Dither*, a `Choice` row whose note says what A and B are for that test). For a
  setting A is what the engine's `OutputSettings` say (`Player::output_settings`), shown as a
  reading, and B is chips of the values the listener picks (`Chosen`, one `Option` per
  setting): restoration, the resampler's quality and phase, the dither's kind and noise shaping,
  each defaulting to the CLI's opposite (extend or off, fast or very high, none or triangular; phase
  and shaping the settings'). `Chosen::contest` is the `Contest` and nothing else decides it. The
  test (`ListeningTest`, a `Session` and a `Subject`: the equaliser's `Arms` and sink, or a
  `Contest` and its `Lowered`) begins on the background executor (`begin_the_audition`): restoration
  measures the song through both arms (`Player::levels_of_the_contest`, with a stop flag the
  `Measuring` handle sets when it is dropped), then `Command::Audition` of A is requested and its
  answer waited for (`AUDITION_BEGUN_WITHIN`, 10 s), so a refusal begins nothing. While it runs the
  begin buttons give way to a greyed *Measuring…* (*Starting…* for the others, which need no
  measuring) and a *Stop*, which drops the handle and sends `Audition(None)`. A refusal is a toast
  (`abx::refusal`) in the window's wording: `NothingToAudition` *Play a song first…*, `ArmsAlike`
  *A and B would sound the same on this stream…*, `LevelsUnmeasured` and `LevelsTooFarApart` that
  the test cannot level them, anything else `toast::would_not_do` of `Cause` (so a
  `Cause::NothingToCompare` reads as it does for any command). While a setting is heard the pane
  says how its levels were matched (*Both play at one level* for the resampler and dither,
  *Measured through both…* or *B lowered by 0.12 dB to match the other* for restoration) in place of
  the equaliser's residual; hearing an arm sends the `Audition` with that arm and ending sends
  `Audition(None)` (the equaliser's end is `tell_the_engine`)
  (`each_test_is_the_contest_of_the_settings_against_the_arm_chosen`,
  `a_test_of_a_setting_with_nothing_playing_is_refused_in_a_toast_and_begins_nothing`,
  `a_test_being_measured_shows_it_and_can_be_stopped_before_it_begins`).
- **`resonate abx FILE [--trials N]` runs the same session on stdin keys** (`crates/resonate/src/abx.rs`):
  one song, repeated by the track (`RepeatMode::Track`), on the real output; it waits for the first
  `OutputChanged` to learn the sink and the negotiated rate, takes the profile the settings bind to
  that sink (`Equalisation::for_sink`, `equaliser::resolved`; none is `Error::AbxNothingBound`, no
  output within `OUTPUT_WITHIN` is `AbxNoOutput`, several songs `AbxSeveralSongs`), levels it with
  `Arms::matched` at that rate (a refusal is the core error) and seeds `Session` from the clock
  (`--trials` 1 to 100, 16 by default). Keys are single characters (`input::strokes`: one byte a key,
  escape sequences swallowed whole, a line at a time when stdin is not a terminal): `a`, `b`, `x`
  hear an arm through `Equalisation::auditioning`, `1` and `2` say X is A or B, `?` helps, `q`
  quits. Saying which X was moves to the next trial on the spot and prints the verdict with the
  running score (no X is ever named); the last trial, `q`, end of input or a signal ends the run with
  the score, the chance by guessing and the p-value, and the real `Equalisation` is sent back.
  `Test` is the pure half and holds the tests (`hearing_x_is_the_arm_the_session_drew_for_this_trial`,
  `answering_every_trial_rightly_ends_on_a_significant_score`).
- **Restoration, the resampler and dither are tested by an audition: two chains run side by side
  and the switch is the equaliser's splice.** `Command::Audition(Some(Audition { contest, heard,
  lowered }))` (`resonate-engine::audition`): a `Contest` names what A and B differ in
  (`Restoration { a, b }`, `Resampling { a, b }` of quality and phase, `Dithering { a, b }` of kind
  and shaping) and writes an arm's settings into an `EngineConfig` (`Contest::apply`). Beginning
  one keeps the contested settings as set (`Contested`), writes the heard arm's into the engine's
  config and rebinds where the song stands (one gap, at the start of the test); `bind` then hands
  the new `Output` the other arm (`Output::take_up_the_audition`, `engine/arms.rs`): its plan is
  `plan_for` the other arm's config on the very stream the heard arm opened, its chain built there
  on the engine thread with a `Recall` of what it renders, sized as the splice's. Every block the
  heard chain consumes is widened once and fed to both (`convert`; a transparent heard plan takes the
  converting path while an audition stands, so the other arm is fed too), each arm's output counted
  on the ring's heard frames from the output's first, so the two are aligned frame for frame
  (every stage keeps a track its length from its first frame). A switch of `heard` with the same
  contest and lowering is `Output::hear_the_other_arm`: at `RingProducer::splice_point` it takes
  the other arm's kept frames from the point on, blends the first `SPLICE_BLENDED_OVER` from the
  words written, writes them behind the ring (`write_spliced`) and publishes the ring's `splice`,
  then swaps chain, plan, trim and recall between the arms; nothing is rebuilt and nothing is added
  to the callback. A switch while a splice waits for the graph or a boundary waits is owed
  (`switch_owed`), and where none can be made (DoP, a draining tail, a ring holding too little, a
  chain swapped since) it rebinds. **Two arms the plan and the chain render alike are refused**
  (`Contest::renders_alike`: `OutputPlan::renders_alike`, and the quality and phase under a
  resampler, the dither's kind under a dither, which the chain reads itself; `Error::ArmsAlike`), as
  is nothing playing or a DoP stream (`NothingToAudition`); both read `Cause::NothingToCompare` or
  `NothingPlaying`. While an audition stands the published settings are the ones set, not the arm's,
  and a switch leaves `OutputStatus::mode` the one the audition began on (so no pane or chip
  names X); a volume change sets both arms' gain, a curve edit both arms' bands, and any
  other change of shape rebinds with both arms built again; a setting command for one of the five
  contested settings ends the audition first and is then taken as ever. An equaliser splice is not
  made while an audition stands, nor over a recall a switch between unlike fronts spoiled, until a
  seek starts it over. `Command::Audition(None)` puts the settings back; where the arm as set is
  heard unlowered it lets the other go with no gap (splicing to it first where the other is heard),
  else it rebinds.
- **Restoration's arms are level-matched by measuring them; the resampler's and dither's are flat.**
  `Contest::is_flat_in_level`. `levels_of(sources, item, stream, config, contest, stop)`
  (`Player::levels_of_the_contest` for the row playing, at the negotiated stream and the settings
  as set) decodes the song whole through both arms' chains (`MEASURED_IN_BLOCKS_OF`, at unity
  volume and gain) and meters each with `resonate_analysis::LoudnessMeter` (BS.1770, the study's
  meter), leaving the first `SETTLING_LEFT_UNMEASURED` (4 s, at most half the song) out because
  restoration learns its wall and long-term spectrum over that long and a listener compares what
  follows; `ContestLevels::lowered` lowers the louder arm by the difference (`Lowered`, a `Trim`,
  `Error::LevelsTooFarApart` past `Trim::WIDEST_MILLIBELS`, `LevelsUnmeasured` for silence or a
  stop). The trim scales that arm's input as it is widened (`Output::level_the_arms`), before
  every stage and the gain the ring is told of, so the ring's level arithmetic is untouched and
  dither still lands on the grid. Proved on the fake graph in `tests/transport.rs`: two resampler
  qualities and dither on and off switched three times with no period losing the sine's peak, no
  step past the sine's own and no reopen, the undithered arm coming out the same words every period
  and the dithered one not
  (`switching_between_two_resampler_qualities_splices_the_other_without_a_gap_or_a_step`,
  `switching_dither_on_and_off_splices_the_other_without_a_gap_or_a_step`); restoration off and
  extended over an MP3 of steady tones, measured some 0.12 dB apart, heard within 0.02 dB of each
  other once lowered, with no gap and no reopen
  (`restoration_on_and_off_are_heard_level_matched_and_switched_without_a_gap`, skipped without
  ffmpeg's libmp3lame).
- **`resonate abx FILE --of restoration|resampler|dither` tests a setting** (`contest_of`): A is
  what the settings and flags say, B the `--against-restoration`, `--against-quality`,
  `--against-filter-phase`, `--against-dither` or `--against-noise-shaping` named (by default the
  opposite: extend or off, fast or very high, none or triangular; phase and shaping the settings'),
  and an `--against` flag of another test is `Error::AbxAgainstElsewhere`. Restoration is measured
  before the test begins (on the terminal's thread; a signal leaves as ever); the keys and session
  are the equaliser's, `a`, `b`, `x` sending the `Audition` with that arm, and the end sends
  `Audition(None)` (`Test::putting_back`;
  `a_setting_is_tested_as_the_settings_have_it_against_the_arm_named_or_its_opposite`,
  `an_arm_named_for_another_test_is_refused`,
  `hearing_an_arm_of_a_setting_sends_the_audition_with_that_arm_and_ending_lets_it_go`).
- **The phones reach it through `resonate-mobile/src/abx.rs`**: `listening_test_lengths()` (8, 16 and
  32, the pane's), `begin_listening_test(curve, trials)` makes a `ListeningTest` object (the
  `Session`, the `Arms` and which arm is heard) from the curve shown, levelled at the stream's rate
  (48 kHz with none open); a curve with no bands and a unity preamp is `ListeningTestNothingToTest`,
  trials outside 1 to 100 `ListeningTestTrials`, a curve that will not level `ListeningTestLevels`.
  `hear_in_listening_test(test, A|B|X)` sends `SetEqualisation` of the arm as the fallback, `enabled`
  whatever the switch says; `ListeningTest::answer(A|B)` answers whether it was right (a finished
  test is `ListeningTestFinished`) and `standing()` is the whole `TestStanding` (planned, answered,
  right, finished, `chance_percent`, `significant`, the verdicts, the levels and the arm heard,
  none for X); `end_listening_test()` sends the preferences' own `Equalisation` back
  (`PlaybackPreferences::equalisation`, the one `commands` builds). Both phones draw it in the
  equaliser page after the room correction with the desktop's wording (Android `ListeningTestSection`,
  iOS `ListeningTestSection`; the words are `ListeningTestView.kt` and `ListeningTestWords.swift`,
  held by `ListeningTestViewTest` and `ListeningTestWordsTests`) and end it when the page is left or
  the test is ended; an edit of the curve sends the real curve and so ends the comparison in all but
  name. **A setting is tested through `begin_setting_test(setting, trials)`** (blocking: restoration
  is measured first, so a phone calls it off its main thread): a `TestedSetting` (`Restoration`
  of two `LossyRestoration`s, `Resampling` of two qualities and phases, `Dither` of two
  `TestedDither`s: `Off`, `Flat` triangular, `Shaped` triangular under `Threshold`) becomes the
  engine's `Contest`, the test holds it with its `Lowered` (`Compared::Setting`, its
  `residual_db` 0) and begins the audition at A, refused as the engine refuses
  (`Error::Player`, `a_setting_test_is_refused_while_nothing_plays`); hearing an arm sends the
  `Audition`, and `end_listening_test()` sends `Audition(None)` before the preferences' curve.
  `TestStanding.levels` is a `TestLevels` (`Matched { residual_db }` for the equaliser, `Alike` for a
  flat setting, `MeasuredAlike`, `Lowered { arm, db }`), which replaced the equaliser's bare
  residual so a front end words how each test was levelled; `setting_test_by_default(kind,
  preferences)` makes the `TestedSetting` a phone begins from, A the preferences' restoration,
  quality and phase (dither as the engine plays it, triangular under `Threshold`, which the phones
  never change: `TestedDither::Shaped`) and B their opposite, so no front end keeps the rule. An
  engine refusal reads as the sentences above (`failure::of_a_player`, so `Error::Player` shows
  *A and B would sound the same…* where it showed only the cause). Both phones' *Listening test*
  sections choose *Equaliser*, *Restoration*, *Resampler* or *Dither* (`Testing`, `ListeningTesting`),
  show A and chips or a menu for B (A is left out of B's restoration and dither choices, which would
  render alike), call `begin_setting_test` off the main thread with *Measuring…* on the begin
  buttons for restoration and *Starting…* for the rest, and word the levels as the window does
  (`ListeningTestViewTest`, `ListeningTestWordsTests`).

## Room correction

- **A measured impulse response is convolved before the equaliser.** `Impulse`: per-channel taps at
  the measured rate (mono serves all; fewer channels than the stream wrap round); `Impulse::at`
  redraws at the stream rate via the `VeryHigh` resampler, cached per rate, time-aligned
  (`a_response_taken_at_another_rate_keeps_its_peak_where_it_was_in_time`). `Impulse::new` refuses a
  non-finite tap. `engine::read_impulse` decodes any codec-readable file, at most `LONGEST_IMPULSE`
  (10 s), **with headroom**: `Impulse::with_headroom` scales every channel by the reciprocal of the
  greatest magnitude where above unity, so a dip-boosting correction never clips; a quieter response
  is never raised. `Convolver`: uniformly partitioned overlap-save over `realfft` (real
  input, `partition + 1` bins a spectrum), one partition behind (`latency_frames`); a response's
  spectrum is made once a distinct response channel and shared by `Arc` across the stream's channels
  that wrap onto it (a mono response is transformed once); the history ring walks newest first by
  splitting at the head, no `%` per partition; `Impulse::loudest_gain` transforms on a 2-3-5-smooth
  even length of at least four points a tap, not a power of two. **A response is bounded in bytes**:
  `RESPONSE_BYTES_AT_MOST` (512 MiB) refuses with `Error::ResponseTooLarge` both a redraw at another
  rate (`Impulse::at`, taps held) and a `prepare` whose filter and history spectra would pass it,
  before any is allocated; `partition_frames_at` grows it with the rate (flat cost); a track's end
  flushes the tail.
- **`EngineConfig::convolution`, `Command::SetConvolution`, key `convolution`.**
  `OutputPlan::convolution` makes the plan `Converted`; a change rebinds at the position, no retune.
  The binary reads the file at start, `explain` names it; the pane's *Room correction* group reads
  one on a background thread. `RootView::reading_the_room` is one task: a second pick replaces it,
  *Stop correcting* drops it, so the last gesture wins. One response for all devices, no binding.
- **The fit of the preamp covers it** (*Fit the preamp to the music*): the decode runs the
  `Convolver` before the stage and the bound multiplies its response in.
- **The phones keep one response file** (`resonate-mobile/src/room.rs`): `keep_room_correction(bytes,
  extension)` refuses an extension outside `AUDIO_EXTENSIONS` and `HRIR_EXTENSIONS` (`ImpulseExtension`) or more than
  `IMPULSE_BYTES_AT_MOST` (64 MiB, `ImpulseTooLarge`), stages the bytes as
  `room-correction-candidate.*` in the player's folder, reads them back through `read_impulse`
  over the local provider, and only a response that reads replaces any `room-correction.*`; one
  that holds no response (`NoImpulse`) is dropped and the kept response stands; `apply_room_correction(on)` sends `SetConvolution` with
  the kept response or none, answering its channels, rate and length; `kept_room_correction` answers the same description without sending anything, so a page shows the response with correction off; `forget_room_correction`
  clears and deletes it. iOS keeps `room-correction` (on) and `room-correction-name` in the
  `playback` suite and applies the response at start, a missing file turning correction off with a notice (`RoomCorrectionState`, the equaliser page's
  *Room correction* section, `fileImporter` of audio); Android keeps the same two keys in its
  `playback` preferences and does the same (`RoomCorrectionState.kt`, `RoomCorrectionSection`, the
  system audio document picker).

## Crossfeed and binaural responses

The stereo image, apart from the transducer: both are stereo-only, off by default and make the plan
`Converted`.

- **Crossfeed is a Bauer-style stage** (`resonate-dsp/src/crossfeed.rs`, `Crossfeeder`): each ear gets
  the other channel delayed by `DELAY_SECONDS` (300 µs, rounded to whole frames at the rate) and
  lowpassed by a one-pole filter, mixed with the channel's own. `Crossfeed` (`off`, `light`,
  `moderate`, `strong`; key `crossfeed`) names a cutoff and a feed level, the figures bs2b's presets
  use: light 650 Hz and 9.5 dB (Meier), moderate 700 Hz and 6 dB (Chu Moy), strong 700 Hz and
  4.5 dB (bs2b's default). **Level-compensated by construction:** the cross gain is `b = 10^(-level/20)`
  over the direct gain 1, and both are divided by `1 + b`; the lowpass has unity gain at DC and a
  non-negative impulse response, so the sum of absolute gains is one and no output sample passes the
  largest input one (`nothing_the_stage_makes_passes_the_largest_sample_it_was_given`). A steady
  mono signal and a low mono tone keep their level (the bass is common to both ears); the treble of
  a mono signal falls by `1 + b`, which is the price of that guarantee. State is a delay line and
  one lowpass word a side, sized in `prepare`; `process` allocates nothing. `prepare` refuses
  anything but stereo (`ChannelMismatch`); `crossfeed_config` plans it only on a stereo stream, after
  the remix, so a mono or 5.1 stream is left alone and a mono source on a stereo sink is crossfed
  (and unchanged, being common to both).
- **Where it runs:** first in the *back* (`OutputPlan::backed`), after the convolver and the
  resampler, before the equaliser, so a headphone correction still has the last word on the
  transducer. `SetCrossfeed` rebinds the plan at the position as `SetConvolution` does
  (`rebind_where_the_plan_moves`): no splice, so it is heard a ring later. A DoP stream refuses it
  (`dop_survives`). The binary reads `crossfeed` from `config.toml`, the settings pane has a
  *Crossfeed* group in Processing, `resonate explain` prints it. **The phones offer it as a preference**:
  `PlaybackPreferences::crossfeed` is a `CrossfeedLevel` (`off`, `light`, `moderate`, `strong`, the
  engine's four) and `commands` sends `SetCrossfeed` when it moves. iOS and Android keep it under
  `crossfeed` beside the other playback preferences (the level's name, `PlaybackPreferencesStore`),
  and their *Sound processing* pages offer it in a *Stereo image* section with the desktop's wording
  for each level and its note that only a stereo stream is crossfed and a crossfed stream is converted
  (`a_crossfeed_level_is_sent_once_and_survives_a_round_trip_of_every_level`, `StoresTests`).
- **A two-by-two response is the same convolver over four taps.** `Impulse::crossed` takes exactly
  `CROSSED_RESPONSES` (4) channels in the order left-to-left, left-to-right, right-to-left,
  right-to-right, where *XY* is input *X* heard at output *Y* (a pure cross response swaps the
  channels, `a_pure_cross_response_swaps_the_channels`; the identity is transparent). `read_impulse`
  makes a crossed `Impulse` of any four-channel file, so the *Room correction* file serves as a
  binaural response; `Impulse::is_crossed` is part of equality. **Two stereo files make the same
  response**: `Impulse::binaural(left, right)` takes the left speaker's response (its channels are the
  two ears, LL and LR) and the right speaker's (RL and RR) and refuses what is not a pair with a typed
  `resonate_dsp::Error` (`SpeakerNotStereo`, `SpeakerAlreadyCrossed`, `SpeakerRatesDiffer`,
  `SpeakerLengthsDiffer`). `engine::read_binaural_pair` reads both files unscaled, joins them and only
  then applies `with_headroom` to the whole (scaling each file alone would change their balance), and
  `binaural_wave` writes the result as a four-channel float WAVE, which is how a pair is kept as the one
  file the settings and the phones already name. The desktop's *Room correction* group has *Choose a
  pair…* (left file, then right), which keeps `binaural-response.wav` beside the equaliser folder and
  corrects by it as by any chosen file; the phones' boundary has `keep_binaural_room_correction`
  (two byte strings and extensions, a refusal `Error::ImpulsePair`), which keeps the joined pair as
  the one `room-correction.*` response. Each phone's *Room correction* section offers *Choose a
  pair* (the left speaker's file, held while the section shows *Left speaker* with *Choose the right
  speaker's response* and *Cancel*, then the right's), reads both as the single file is read, keeps
  them through it and switches correction on, naming the response *<left stem> and <right stem>*
  (`pairedRoomName`, `RoomPair.name`); a refusal is reported as the single file's is, and the pair
  held is cleared whatever the answer.
  Every pane says when the response held is two-by-two: the desktop group a note under the file, the
  phones' description *Two-by-two (binaural)* for four channels
  (`two_stereo_responses_are_assembled_left_speaker_first`,
  `a_pair_that_is_not_two_stereo_responses_of_one_rate_and_length_is_refused`,
  `two_stereo_responses_are_kept_as_one_two_by_two_response_that_reads_back`). `Convolver::prepare` builds four
  `Channel`s (input *k / 2* to output *k % 2*) only when the impulse is crossed **and** the stream is
  stereo; on any other layout the four channels are taken one a stream channel as before. Each
  output sums two channels' ready samples, so the cost is that of four mono convolutions. **Headroom
  is the larger of each ear's summed magnitudes** (`crossed_gain`: `|LL| + |RL|`, `|LR| + |RR|` at
  each bin), so `with_headroom` holds coherent input at both ears under unity. `Impulse::at`
  resamples each of the four alike. The preamp fit's `Applied` still reads a correction channel by
  channel (the loudest pair), so the model understates a crossed response's sum; the decode
  (`peak_through`) runs the real convolver and is exact.

- **A SOFA set of head-related responses is read as the same two-by-two response**
  (`resonate-codec/src/sofa.rs`, `read_hrir(sources, location, longest, angle)`; `HRIR_EXTENSIONS`
  is `sofa`, apart from `AUDIO_EXTENSIONS` so a scan never admits one as a song).
  `engine::read_impulse` (and `read_binaural_pair`) take the `Speakers` and ask `read_hrir`
  first, falling through to the decoders where the file is neither HDF5 nor netCDF-3 classic
  (`Ok(None)`: sniffed from the first bytes, the HDF5 signature or `CDF` and a version, never the
  extension), so the desktop's file choosers, the `convolution` key and the phones' pickers need no
  path of their own; a fault is a typed `codec::Error::Sofa { location, source: SofaFault }`, and
  `read_binaural_pair` refuses a set as a pair's half like any crossed response.
  **The container is either of two**: HDF5, by `hdf5-reader` (pure Rust, read only,
  `default-features = false`: no rayon, no lz4) with the chunk cache held to 16 MiB and a chunk to
  64 MiB, float32 and float64 contiguous or chunked with deflate and shuffle; or netCDF-3 classic,
  CDF-1, CDF-2 (64-bit offsets) and CDF-5 (64-bit data: 64-bit counts and lengths, 32-bit
  dimension ids, the unsigned and 64-bit integer types, which a SOFA set never reads as numbers), by
  `netcdf.rs`, a reader of our own (no C library):
  `Classic::of` walks the header (dimensions, attributes, variables), every count checked against
  the bytes left before anything is allocated, every name text, every dimension named by a variable
  declared, the record dimension first; `Variable::numbers` takes a `Pick` (an index or a span) a
  dimension and reads only the elements picked, big-endian float and double, a record variable
  stepping by the record size, every offset checked against the file (`ClassicFault`); a `CDF`
  file of another version is `SofaFault::ClassicVersion`. The one `Variable` enum of
  `sofa.rs` serves both, so a set is read by the same code from either. **An HDF5 set from a
  seekable source is read by ranges, not whole** (`Container::streamed`): the stream goes behind
  `hdf5-reader`'s `RangeRequestStorage` (a seek and a `read_exact` of the range asked, a range past
  the file or over `STREAMED_READ_AT_MOST`, 64 MiB, refused before it is allocated), so a set is
  held only as the superblock, headers, chunk index and the measurements picked are, up to
  `SOFA_STREAMED_BYTES_AT_MOST` (8 GiB, `SofaFault::TooLarge`); a netCDF-3 file, or a stream that
  cannot seek, is one `Vec` of at most `SOFA_BYTES_AT_MOST` (128 MiB). Reading `Data.IR` takes only the
  measurements chosen, as hyperslabs or runs, never the whole set
  (`a_seekable_set_is_read_from_the_measurements_chosen_alone_and_a_stream_that_cannot_seek_is_read_whole`
  counts the bytes the source served: under a quarter of the file), and every file-declared shape is
  checked before a read (`Data.IR` exactly `[M, 2, N]`, or `[M, 2, E, N]` for a speaker set,
  `SourcePosition` exactly `[M, 3]`, `EmitterPosition` `[E, 3]` or `[E, 3, 1]`, `ListenerView` `[M, 3]`
  or `[1, 3]`, `ListenerPosition` `[M, 3]` or `[1, 3]`, `Data.Delay` `[2]`, `[1, 2]` or `[M, 2]`, `Data.SamplingRate` at most a million
  numbers, `M` at most a million, `E` at most 65 536, the response cut at `LONGEST_IMPULSE`).
  **Where the speakers stand is three settings, one `Speakers`** (`resonate_core::eq`): a
  `SpeakerAngle` (whole degrees, 5 to 90, `STANDARD` 30), a `SpeakerElevation` (whole degrees,
  -30 to 60, `LEVEL` 0, `Error::SpeakerElevationOutOfRange`) and a `HeadYaw` (whole degrees, -90 to
  90, `AHEAD` 0, positive to the left, `Error::HeadYawOutOfRange`), the keys `speaker-angle`,
  `speaker-elevation` and `head-yaw` (`Config::speakers`, each an integer held to its range, a value
  outside it costing the key alone), `Setting::SpeakerAngle`, `Setting::SpeakerElevation` and
  `Setting::HeadYaw`, worn on `ResonateApp::speaker_angle`, `speaker_elevation` and `head_yaw`
  (`speakers_worn` in the pane makes the trio every read takes). The Room correction group offers each as a stepper (`degrees_stepper`: a
  `kit::icon_button` of `Icon::Minus` and one of `Icon::Plus`, a degree a press, the reading
  between) beside chips for common places (20, 30, 45, 60 and 90 degrees of angle; 0, 10, 20, 30
  and 45 of elevation), so any angle from 5 to 90 and any elevation is reached, and a third field,
  *Head turned* (`head_yaw_field`, the same stepper with chips at -45, -20, 0, 20 and 45 degrees);
  changing one stores it and reads the held response file again (`set_the_speaker_angle`,
  `set_the_speaker_elevation`, `set_the_head_yaw`, then `correct_the_room`), the group's mark puts
  back 30, 0 and 0 and stops correcting, and every
  read of the response the window makes (the shape look, the fit of the preamp, a pair) asks at the
  trio worn. The left speaker is `angle` degrees left of the front at `elevation` degrees above
  the ears, the right the same to the right; **a head turned `yaw` degrees to the left sees each
  speaker `yaw` degrees round the other way**, so a free-field set is looked up at the speaker's
  azimuth less the yaw (`a_head_turned_moves_both_speakers_round_by_the_turn_before_the_set_is_read`). **A free-field set** (`SimpleFreeFieldHRIR`) reads
  each speaker from the measurements around it (`neighbours_of`): the nearest, by the angle between
  directions (the radius is ignored), must lie within 45 degrees (`NoMeasurementNear`, naming the
  side, else the set is refused rather than answered with a far one); within
  `EXACT_WITHIN_DEGREES` (0.5) it is taken whole, the first of equals winning. **Otherwise the
  speaker is interpolated inside a triangle of measurements** (`surrounding`): the
  `TRIANGLE_CANDIDATES_AT_MOST` (16) nearest measurements within `TRIANGLE_WITHIN_DEGREES` (30) are
  projected onto the plane touching the sphere at the speaker's direction (gnomonic, so a great
  circle is a straight line), every triple of them holding the speaker (barycentric weights all
  non-negative, area over `TRIANGLE_AREA_AT_LEAST`) whose circumscribed cap holds no other
  candidate (the Delaunay condition on the sphere, `cap_around`) is a triangle of the
  triangulation, the smallest cap winning a tie, and the three barycentric weights (a vertex the
  speaker lies on the far side of, weight zero, is left out) are the blend's. Where no triangle
  surrounds it (a set at one elevation alone, a direction past the outermost row, fewer than three
  measurements, collinear ones) the speaker is interpolated **along the ring** the nearest
  measurement stands on (`along_the_ring`): the measurements within `RING_ELEVATION_WITHIN_DEGREES`
  (1) of its elevation (not within `RING_POLE_ELEVATION_DEGREES`, 80, of a pole), the nearest either
  side of the speaker's azimuth, weighted linearly by azimuth distance, if the two stand no more
  than `RING_GAP_AT_MOST_DEGREES` (60) apart; a set at one elevation alone, and a direction past
  the outermost row (along that row), are interpolated so
  (`a_set_at_one_elevation_is_interpolated_between_its_two_neighbours_along_the_ring`,
  `a_speaker_past_the_outermost_row_is_interpolated_along_that_row`,
  `a_ring_with_a_gap_wider_than_sixty_degrees_is_not_interpolated_across`). Only where there is no
  neighbour on one side of the ring (or the gap is wider) the last fallback stands: up to three
  nearest, the nearest always and the others only within `NEIGHBOURS_WITHIN_DEGREES` (15), blended
  by inverse-square distance.
  **A free-field blend is of magnitude and delay apart** (`magnitude_and_delay`): each ear of each
  measurement (with its `Data.Delay` laid as silence before it) has its onset (`onset_of`, the first
  tap at a tenth of its peak); the blend's onset is the weighted mean of them, rounded; the magnitude
  spectrum is the weighted mean of the measurements' magnitudes on a transform four times the
  longest response (`blended_magnitude`), and the response is rebuilt from it as a minimum-phase
  one (`minimum_phase`: the real cepstrum of the magnitude held `SPECTRUM_FLOOR` (1e-6 of the
  loudest bin) above zero, folded, exponentiated, transformed back) and laid with its own onset on
  the blend's. It replaced the time-domain sum for a free-field set because a sum of responses
  whose onsets the threshold reads a fraction of a sample apart cancels toward the top of the band:
  on a synthetic coarse set (thirty degrees between measurements; a one-pole low-pass of moving
  cutoff behind a windowed-sinc fractional delay) the log-spectral distance from the true response
  between them to 16 kHz is 5.5 dB for the sum and 0.18 dB for the rebuild (1.4 and 0.09 at a
  fifth of the way)
  (`a_coarse_set_blended_by_magnitude_and_delay_lands_nearer_the_head_between_than_a_sum_does`); a
  flat magnitude rebuilds to the same impulse, so every impulse test holds. **A speaker set is
  still summed with onsets aligned** (`aligned_sum`): a room's reflections are not minimum phase
  and a rebuild would pull them forward. In the sum each ear of each measurement (with its
  `Data.Delay` laid as silence before it) has its onset, the first tap at a tenth of its peak
  (`onset_of`); the blend's onset is the weighted mean of them, rounded, and each response is
  shifted to it before the weighted sum, so impulses of different delay add into one sharp impulse
  and not a comb, while the arrival time (and so the interaural delay) follows the weights
  (`two_measurements_either_side_of_a_speaker_are_blended_with_their_onsets_aligned`,
  `a_blend_keeps_the_difference_between_the_ears_that_the_delays_make`). **A speaker set**
  (`MultiSpeakerBRIR`, `Data.IR` of four dimensions) is read at one listener view and its emitters
  interpolated like measurements: the view (`ListenerView`, spherical or cartesian, cartesian where
  it names no `Type`) nearest the head's heading (`HeadYaw` degrees from the front) is the one read,
  and the head's turn from that view's own azimuth (all of it where there is one view or none
  named) moves the lookup the other way; each speaker is then asked of the emitters
  (`EmitterPosition`) as a measurement is (`neighbours_of`: the exact one, a triangle, the ring or
  the nearest), within 45 degrees or `NoMeasurementNear`
  (`a_speaker_between_two_emitters_takes_both_by_where_it_stands`,
  `a_head_turned_in_a_room_is_read_at_the_view_it_faces_and_what_is_left_of_the_turn_moves_the_lookup`).
  **The listener stands where `ListenerPosition` says** (`[1, 3]`, or the `[M, 3]` row of the view
  read; cartesian where it names no `Type`, metres): an emitter's direction is its position less the
  listener's, taken with its radius (`Coordinates::point`; a spherical emitter's third coordinate),
  and a listener at the origin, or none, leaves the directions as the emitters' own
  (`a_listener_off_the_origin_hears_each_emitter_from_where_it_stands`). `SourcePosition:Type` (and the
  emitter's, the view's) is `spherical` (azimuth counterclockwise from the front, elevation, in
  degrees unless `Units` names radians) or `cartesian` (front, left, up), anything else
  `PositionTypeUnknown` and a missing `Type` `MemberMissing` (not for the view). The left speaker's
  response gives LL and LR (receiver 0 and 1, the left and right ear), the right speaker's RL and
  RR, the order `Impulse::crossed` takes. A `Data.Delay` (samples, rounded, negative taken as
  none) is laid as silence before the ear's response, and the four responses are padded with
  silence to one length. The rate is `Data.SamplingRate` rounded to hertz (`RateNotUsable` outside
  8 kHz to 768 kHz); the response is then redrawn at the stream's rate by `Impulse::at` like any
  other, and `with_headroom` holds the whole under unity. `fuzz/fuzz_targets/sofa.rs` feeds
  `read_hrir_bytes`, seeded by a plain and a deflated set
  (`every_seed_of_the_fuzz_target_is_a_set_that_reads`); fixtures are built by `hdf5-writer` and,
  for netCDF-3, by the test-only `netcdf::written` in the tests
  (`the_measurement_nearest_each_speaker_is_the_one_whose_taps_are_taken`,
  `a_speaker_angle_the_listener_sets_chooses_the_measurements_at_that_angle`,
  `a_classic_netcdf_file_reads_as_the_same_set_in_hdf5_does`,
  `a_classic_file_keeping_its_measurements_as_records_single_precision_and_delayed_reads_too`,
  `a_set_of_speakers_is_read_at_the_view_facing_the_front_and_the_emitters_at_the_angle`,
  `cartesian_positions_choose_as_spherical_ones_do`, `a_delay_puts_silent_samples_before_an_ear_and_every_response_stays_as_long_as_the_rest`,
  `a_set_cut_short_or_marked_anywhere_is_refused_or_read_and_never_panics`,
  `a_classic_file_cut_short_or_marked_anywhere_is_refused_or_read_and_never_panics`,
  `a_set_of_head_related_responses_is_read_as_the_two_by_two_response_of_the_speakers`,
  `a_speaker_angle_the_listener_sets_moves_the_measurements_a_set_is_read_at`,
  `a_speaker_inside_a_triangle_of_measurements_takes_all_three_by_where_it_stands`,
  `a_speaker_standing_nearer_one_corner_of_its_triangle_takes_more_of_that_corner`,
  `a_speaker_raised_is_read_from_the_measurements_at_that_elevation`,
  `a_speaker_between_a_row_of_measurements_and_the_next_row_above_is_blended_of_both_rows`,
  `a_set_of_head_related_responses_is_kept_as_the_two_by_two_response_it_reads_as`,
  `a_netcdf_version_this_reader_does_not_know_and_a_header_that_lies_are_refused`, with CDF-5 in
  `netcdf.rs`'s `wide_offsets_and_wide_data_read_as_narrow_ones_do`). The phones'
  pickers offer `.sofa` beside audio (Android's `RESPONSE_DOCUMENTS`, iOS's `responseTypes`) and
  keep one as `room-correction.sofa`; the boundary holds the pair (`speaker_angle`,
  `set_speaker_angle(degrees)`, 30 until set, an angle outside 5 to 90 a `Domain` error;
  `speaker_elevation`, `set_speaker_elevation(degrees)`, 0 until set, outside -30 to 60 a `Domain`
  error; `head_yaw`, `set_head_yaw(degrees)`, 0 until set, outside -90 to 90 a `Domain` error) and
  reads the kept set at it, re-reading and re-sending the correction applied when
  either moves (a set that will not read at the new place leaves the pair and the correction
  standing). `speaker_limits()` hands the front ends the bounds and the standard angle
  (`SpeakerLimits`, the yaw's included), so neither phone keeps a number of its own. A phone still
  refuses a set past `IMPULSE_BYTES_AT_MOST` (64 MiB): that is the bytes crossing the boundary,
  not the reader (`FEATURES.md`). **Both phones' Room correction
  sections offer the three**: Android a `SpeakerSetting` each (a `ListItem` with the reading and
  `IconButton`s of `Icons.Rounded.Remove` and `Add` stepping a degree, then `FilterChip`s of the
  desktop's presets; the head's turn at -45, -20, 0, 20, 45), kept as `speaker-angle`,
  `speaker-elevation` and `head-yaw` in the `playback`
  preferences and sent at start before the response is read (`RoomCorrectionState.restore`); iOS a
  `Stepper` each over the same limits, kept under the same keys in the `playback` suite
  (`the_speaker_angle_a_listener_sets_is_the_one_a_set_is_read_at`,
  `the_speaker_elevation_a_listener_sets_is_the_one_a_set_is_read_at`,
  `the_head_yaw_a_listener_sets_is_the_one_a_set_is_read_at`,
  `the_limits_a_phone_offers_are_the_ones_the_boundary_refuses_beyond`).

## The binding

- **Measured for one pair of headphones, so bound to a device.** `Equalisation` = `enabled`, a
  `BTreeMap` from `NodeName` to profile, a fallback (a map, not a list: a list makes equality
  order-dependent in a value compared sixty times a second). Values behind `Arc`: a publish is a
  refcount bump.
- **The engine resolves, not the pane**: `select_sink` falls back to the system default, so only the
  engine knows the opened sink. `Output::bound` records its `SinkInfo` name, `plan_for` takes it;
  `eq_config` is the one place a name becomes a profile (mirrors `gain_config`). `SetSink` rebinds,
  `Output::open` re-resolves, a default changing under a playing stream is followed
  (`follow_the_sink_it_would_choose`).
- **`config.toml`: a boolean, a binding, a table of devices alone.** `equaliser` on/off;
  `equaliser-profile` the fallback; `[equaliser-for]` maps `node.name` to binding. The fallback is a
  key, not a reserved table entry, so a sink called `default` is bindable
  (`a_sink_named_default_is_bound_on_its_own_rather_than_for_the_rest`). `Bindings::for_sink` says
  *whose* binding it read. A dotted node name is one quoted key; clearing the last entry removes the
  table.
- **A binding is a kept profile or the device's own curve, as a type**: `resonate_eq::Binding` =
  `Profile(ProfileName)` | `Own`, written as the name or `true` (no profile name is reserved).
  `false` is refused in the fallback key, passed over with a warning in the table. A device that
  never shaped an own curve holds a flat one, which `is_transparent` keeps out of the chain.

## The formats

- **Profiles are files, not rows**: `$XDG_DATA_HOME/resonate/equaliser/<name>.txt`, EqualizerAPO
  text, hand-editable, readable by other players; `resonate play`/`resonate eq` work with no
  catalog. **A save edits the text that stands rather than writing the model** (`apo::edited`): the
  walk that reads a profile (`apo::walk`) names each line's role (`Preamp`, `Band(k)`, `Graphic`,
  else `Other`), bands are aligned old to new by longest common run so an edit changes the lines it
  moved (a band inserted or removed lands or goes beside its own comment), and every `Other` line
  (comments, `Include:`, `Device:`, `If:`, a second device's lines) stays where it was, while a
  `Surplus` line (a filter past `MAX_BANDS`, which never played) goes with any save that changes the
  profile, else removing a band would bring the 33rd into play at the next read
  (`removing_a_band_from_a_profile_of_more_than_the_cap_does_not_bring_the_next_into_play`); a band given other channels is wrapped in `Channel:` lines that restore the
  scope after it. The edit is read back and must equal the wanted profile, else one more try
  states the preamp, else the model is written fresh (`apo::write`). A model nothing moved leaves
  the file untouched. Line ending, UTF-8 mark and encoding are the file's own (`Standing`,
  `text::encoded` plus `TextEncoding::byte_order_mark`; a letter the code page lacks falls to
  UTF-8). **An import keeps the source's bytes whole** (`Kept::bytes`, `keep_read`,
  `keep_own_read`); **an export is a save over the kept file the profile came from**
  (`Store::export(profile, standing_in, to)`, `file_of`). Every write stages as
  `.<name>.<pid>-<n>.new` (`naming::named_within`) under `writer::Held` beside its target and
  renames, then syncs the folder; a save sweeps staged names whose writer is `Gone`.
  `ProfileName` ≤ `NAME_AT_MOST` (96) *bytes*, cut by `ProfileName::after` at the last letter they
  hold. `eq --import`/`--fetch` say when a kept name is replaced (`Store::holds`).
  `Store::names`/`owners` keep the first `PROFILES_AT_MOST` (256) by name.
- **An own curve is a profile file no name can reach**: `Store::own`/`keep_own` use
  `own/every-other-device.txt` (fallback) and `own/device-<node.name>.txt`; resolved like a kept
  profile, never listed by `names`. Node name escaped byte by byte (`[A-Za-z0-9._-]` kept, else
  `%XX`); the `device-` prefix keeps a device called `every-other-device` off the fallback's file;
  one too long is `Error::DeviceNotNameable`
  (`no_device_name_reaches_the_file_another_device_or_the_rest_are_kept_in`).
- **One switch for all devices**: `equaliser` is a single key, so the grammar refuses `eq --off
  --for <sink>` and `--on --for <sink>`; `--unbind` lets one device go. `--suggest` reads the sink
  `--sink` or the default names: `--for` is refused beside it.
- **The CLI binds, shapes, forgets an own curve like the pane.** `eq --own` binds the `--for` device
  (or every other device) to its own curve and switches on; beside `--import`/`--fetch` the read is
  kept as that curve, beside `--export` it is written out. `--forget-own` (`Store::forget_own`) takes
  the owner's binding too only where it was the own curve; neither held = `Error::NoOwnCurve`. An
  own curve outlives its binding on purpose. `Store::owners` accepts a stem only where escaping its
  unescaped name rewrites the same stem; `--list` shows own curves so a gone device can be
  forgotten; a too-large or broken file is an `unreadable` row, not the end of the list.
- **A profile is read as far as it parses, never failing on a line** (the `lrc.rs`/`cue.rs` rule):
  `Preamp:` lines sum, as EqualizerAPO sums them (a sum past the widest is held and counted `clamped`);
  parameters by name; `BW` and `S` convert to Q (`BW` in the unit it names: `Oct`, or `Hz` as centre
  over width; unnamed, octaves), comma decimals read, `#` lines are comments. Only `LARGEST_PROFILE`
  (64 KiB of the file's own bytes, weighed before decoding: `Error::TooLarge`) and `LINES_AT_MOST`
  (4096, `Error::TooManyLines`) refuse; a filter past `MAX_BANDS` is passed over and counted. **A
  file correcting several devices is read as the first it names** (`Device: all` or none reaches;
  lines under another device are never summed in:
  `a_file_correcting_two_devices_is_read_as_the_first_of_them`). `read_number` is exported so the
  pane's cells agree. **What a read gave up is said, CLI and window alike**: `Reading`/`Kept` carry
  `passed_over`, `approximated` (a non-second-order rolloff word) and `clamped` (a value held to what
  this build holds).
- **An AutoEq GraphicEQ line is a conversion, and the importer says so.** Its 127 points are fitted
  onto the 31 ISO third-octave centres at `Q::THIRD_OCTAVE`, iterating the bank's response against
  the target `FITTING_PASSES` (12) times (neighbours sum and overshoot). Points interpolate linearly
  in log frequency; endpoints held, not extrapolated. A band under
  `WORTH_A_BAND_MILLI_DECIBELS` (200) is dropped and the rest refitted: a flat curve imports as no
  bands.
- **A fit does not travel: a profile keeps its curve, refitted at the playing rate** (the bilinear
  warp near Nyquist is part of what a 48 kHz fit compensates). In `resonate-core::eq`: `Target` =
  quantised `TargetPoint`s, sorted, de-duplicated, thinned past `TARGET_POINTS_AT_MOST` (1024);
  `Profile::fitted_to` fits at `FITTED_AT` (48 kHz); `Profile::at_rate` gives the fit at another
  rate. `eq_config` asks it for the stream rate before weighing transparency. A `Target` caches fits
  for the eight `FITS_KEPT_FOR` rates (44.1 to 384 kHz) in a `OnceLock`, outside `PartialEq`/`Hash`,
  holding bands not a `Profile` (a cycle through its own `Arc`), since a retune re-plans on every
  volume step. A hand-moved preamp is carried as its distance from the fitted one. A `Target`
  profile is written as `Preamp:` plus the `GraphicEQ:` line, gains trimmed to the milli-dB; filter
  lines beside one are passed over. `band_mut`, `push`, `remove` let go of the curve.

## A measurement of your own

A second correction source beside AutoEq: the listener's own frequency-response measurement, fitted
to a target. `resonate-eq` only, no network, on the desktop, the CLI and both phones.

- **A response is read as far as it parses** (`measurement::read_response`, the `apo` rule): text goes
  through `text::decoded`; `Response` is points sorted by hertz, a repeated frequency keeping its
  first, a frequency outside 0 to 1 MHz or a level past 400 dB or not finite dropped; at least
  `POINTS_AT_LEAST` (8) points or `NoMeasuredPoints`/`TooFewMeasuredPoints`; `LARGEST_MEASUREMENT`
  (4 MiB, weighed before decoding) or `MeasurementTooLarge`; a line that is no point is counted in
  `passed_over`. Three layouts, one reader: REW's export (`*` lines, then frequency, SPL and phase
  split on tabs or spaces, `Layout::Rew`), a headed CSV such as AutoEq's `frequency,raw` (the level
  is the column named `raw`, `spl`, `magnitude`, `level`, `response`, `amplitude` or `gain`, else the
  second, `Layout::Headed`) and plain frequency/decibel pairs (`Layout::Plain`). Fields split on a tab,
  a `;`, a comma with no space beside it where the line has one token, or whitespace, so a comma
  decimal reads (`read_number`).
- **Targets are data**: `BuiltIn::{Flat, HarmanOverEar2018, HarmanInEar2019}` (named `flat`,
  `harman-over-ear-2018`, `harman-in-ear-2019`, any case). The two Harman curves are AutoEq's
  published `targets/Harman over-ear 2018.csv` and `Harman in-ear 2019.csv` (MIT-licensed, 696
  points each, 0 dB at 1 kHz), kept whole as `crates/resonate-eq/targets/*.csv` with its notice beside them (`LICENSE-AutoEq`, named in `COPYRIGHT`) and read by the same
  reader; the tests pin their bass, ear-gain and treble levels. Another target is a file in the same
  formats: `Store::goal_named` is a built-in's name, else a path.
- **The fit** (`fitting::fit(measured, goal, &Fitting)`): both responses are smoothed to a sixth of
  an octave onto 256 log-spaced points from 20 Hz to 20 kHz (the mean of the raw points in the
  window, interpolating where none lies in it) and set level by their mean over 500 Hz to 2 kHz, so
  the level a measurement was taken at changes nothing; both must reach 500 Hz and 2 kHz
  (`ResponseMissesTheMidBand`). What is wanted is `goal - measured`. Weights are 1 to 6 kHz falling
  to 0.25 at 20 kHz, since a treble dip is mostly the rig. A greedy pass picks from a dictionary
  of peaking bands (a sixth of an octave apart from 20 Hz to 16 kHz, Q 0.5 to 8), low shelves up
  to 1 kHz and high shelves from 1.5 kHz (Q 0.5 to 1), the one whose least-squares gain removes
  most of the weighted error, the gain held to `boost_at_most_db` (6) and `cut_at_most_db` (12),
  Q to `q_least`/`q_most`, at most one shelf of each kind and one band to a kind and frequency,
  at most `bands` (10, never past `MAX_BANDS`); it stops when a band would be under
  `WORTH_A_BAND_DB` (0.3) or remove under 1% of the error. Each gain is then settled on the
  band's exact biquad response and `REFINING_PASSES` (3) passes re-settle every band in turn
  against all the others, nudging its frequency (a 24th of an octave, twice either way) and Q
  (by a factor of 1.4 either way) where that leaves less. Bands are sorted by frequency and the
  preamp is `Profile::fitted_preamp(FITTED_AT)`, the model's own path. `Fitted` carries the
  weighted RMS error before and after; a measurement the greedy pass finds nothing worth a band
  in is `NothingToCorrect`, never a flat profile kept.
- **The result is an ordinary parametric profile**: no `Target`, so it is not refitted per rate
  (its bands are the fit at 48 kHz and the bilinear warp is the same one every parametric
  profile carries) and it is kept as EqualizerAPO text by `Store::keep`/`keep_own`.
  `Store::fit_file(from, goal, fitting)` reads and fits, answering `Measured { fitted, layout,
  passed_over }`; `Store::response_in` reads a file under `LARGEST_MEASUREMENT`.
- **Reachable from `eq --fit-measurement FILE [--target NAME|FILE]`** (default
  `harman-over-ear-2018`), kept like `--import`: named by `--profile` or the file's stem
  (`Store::name_for_import`, a replaced name said), bound by `--for`, or the device's own curve with
  `--own`; it conflicts with every other action. And from the *Measured corrections* group: the
  *Fit toward* chips (`Goal::{BuiltIn, File}`, Harman over-ear 2018 by default,
  `EqualiserModel::aim_at`) and *Fit a measurement…*, which fits and keeps on the background
  executor in `_measured` (own task, like `_imported`), shows the new profile and says how many
  bands, which target and the error before and after. The group shows whether or not online is on:
  nothing in it asks the network.
- **The phones fit it through one boundary function** (`resonate-mobile/src/measurement.rs`):
  `fit_measurement(measurement, target, name)` reads the measurement's bytes with `read_response`,
  the target as `MeasurementTarget::BuiltIn { name }` (`BuiltIn::parse`, any case) or
  `Document { bytes }` (the same reader), fits with `Fitting::default()` and answers a
  `MeasurementFit`: `imported` (an `EqualiserImport` through `equaliser::imported`, so the curve is
  held to the phone's bounds and counted in `clamped`, `converted` false, `passed_over` the
  measurement's lines that were no point), `bands` and the weighted error `off_by_db_before` and
  `off_by_db_after`. It is the desktop's fit of the same bytes
  (`the_phones_fit_is_the_desktops_fit_of_the_same_bytes`). `measurement_offers()` hands the front
  end the built-ins (`name`, `label`, one `recommended`: Harman over-ear 2018, the desktop's default)
  and `bytes_at_most` (`LARGEST_MEASUREMENT`), so a phone reads that and one byte and keeps no list or
  limit of its own. A refusal is `Error::Measurement { file, source }` (`MeasuredFile::Measurement`
  or `Target`, a `ResponseMissesTheMidBand` attributed to the file whose span it carries) displayed
  as a sentence naming the file (`failure::of_a_measurement`), or `UnknownTarget { name }`
  (`a_file_that_will_not_read_is_refused_in_a_sentence_naming_which_file`). The fit is CPU work and
  synchronous, so a phone calls it off its main thread. Neither phone keeps the profile as a file:
  the result becomes the curve in use, as any import does, and is kept as the curve's text.
- **The desktop measures live: `resonate eq --measure`** (`measuring.rs`), then the same fit a file
  goes through. The phones' room correction measures nothing (`room.rs` keeps a response file), so
  the sweep is the desktop's own, the window's included. `resonate_dsp::Sweep` is an exponential sine sweep from 20 Hz to
  20 kHz over `SWEEP_FOR` (8 s) at 48 kHz, 0.1 of full scale (-20 dBFS) with 20 ms Hann fades, refused
  as `Error::SweepBand` (not below half the rate) or `SweepLength` (500 ms to a minute).
  `Listener::play_and_record` (`resonate-listen`, still reaching core, audio and pipewire alone; it
  takes the `Hearing` listening's capture takes, whose `stop` ends the run at once as
  `Error::Stopped` and keeps nothing)
  opens the capture first, then a playback stream of its own on the output `--sink`, the settings
  or the default names (`NoSuchSink`, `NoOutput`), straight to the device: the sweep never passes
  the engine, so the equaliser, convolver and crossfeed in force do not touch it. It is the
  `Listener::record` loop with the playback held beside it (`recorded`), so a lost capture reopens
  as in listening, and records `HEARD_AFTER_THE_SWEEP` (2 s) past the sweep for the room's tail.
  **Deconvolution is regularised spectral division** (`Sweep::heard_as`): the recording's mono mix
  over the sweep's, `Y * conj(X) / (|X|^2 + 1e-6 * max |X|^2)` on a transform at least recorded
  plus sweep long (a recording over 2^24 frames is `HeardTooLong`, shorter than the sweep
  `HeardTooShort`, silent `NothingHeard`). The impulse's peak is found wherever it lies, so the
  latency of output and microphone and their clocks' offset change nothing but `delay_frames`; the
  impulse is gated from 5 ms before the peak to 400 ms after, the last quarter Hann-tapered, which
  cuts the sweep's distortion products (they precede the impulse) and leaves a room's tail past
  400 ms unmeasured. `Heard::response` reads 48 points an octave off a transform of at least
  2^17 bins, and `measuring::measured_by` hands them to `Response::from_points`: a measurement of
  no kind a file lacks. The level is whatever the chain gave; the fit sets it by the mid band.
  **The microphone is taken flat unless a calibration says otherwise**: `--calibration FILE` is read
  as any measurement and taken off the response point by point (`measuring::calibrated`);
  without it the fit corrects the microphone too, which the command's help says.
  `--keep-measurement FILE` writes the response as the headed CSV `frequency,raw`, which
  `--fit-measurement` reads back (`Layout::Headed`); `--microphone [NODE]` names the microphone
  (none: `listen-from`, the desktop's own monitor unless set, which measures the path to the output
  and no room), `--target`, `--profile`, `--for` and `--own` are `--fit-measurement`'s, and the
  profile is `measured` unless named. `measuring::Rig` is the seam (`Room` the real output and
  microphone): a fake output and capture in-process prove the whole path
  (`a_sweep_through_a_fake_output_and_microphone_measures_the_filter_between_them`: a delayed
  one-pole low-pass read within 0.5 dB from 100 Hz to 10 kHz;
  `what_is_measured_live_goes_through_the_fit_a_file_goes_through`;
  `a_microphones_own_response_is_taken_off_what_it_heard`), under the deconvolution's own
  (`a_sweep_through_a_known_filter_is_deconvolved_to_that_filters_response`,
  `the_delay_between_playing_and_hearing_changes_nothing_but_the_delay`, `resonate-dsp`).
  `resonate-listen --test capture` plays a sweep through the desktop's monitor against a real
  daemon and skips without one. **The window's *Measure* is the same path behind a seam**
  (`resonate_ui::Measures`, `Lookups::measures`, filled by the binary's `measuring::Sweeping`; the
  window names no `resonate-dsp` and no PipeWire; `measure(through, from, hearing, calibration)`
  takes the `Hearing` the window stops and the microphone's calibration response, which
  `Sweeping` takes off what was heard as `--calibration` does): `measuring::measured_by(rig,
  hearing)` answers an `Unheard`
  (`Played`, `Deconvolved`, `Response`, each the source error unchanged, converted to the crate's
  `Error` for the command and to `Unmeasured` for the window), and the pane's *Measure your room*
  field (under *Your own measurement*, in *Measured corrections*) holds the microphone to hear it
  through as the Listen sheet's own choice, chips of the desktop, the default microphone and every
  microphone `Listener::microphones` lists (`ListenModel::choose`, shared with the sheet, so a
  microphone chosen in one is chosen in the other; the list is looked for the first time the group
  draws), and *Measure*, which plays through the sink in use (`PlayerModel::sink_in_use`), a *Stop* beside it
  while a run is out (`EqualiserModel::stop_measuring_the_room` stops the `Hearing` the run holds;
  an `Unmeasured` that `is_stopped` is told *The measurement was stopped; nothing was kept*, not as
  a failure), *Choose a calibration…* with the file's name and *Remove* (`EqualiserModel::calibration`,
  read in the run as any measurement by `Store::response_in`; one that will not read is told and
  nothing is played) and a *Keep as* field (`RootView::measured_as`, the profile's name through
  `ProfileName::new`, `measured` where blank, a name refused told in a toast and nothing played).
  `EqualiserModel::measure_the_room(Sounding { rig, through, from, called })` runs the seam, the fit toward the *Fit toward* target and the
  keep as the profile named (`measured`) on the background executor (`_sounded`, apart from the other
  tasks; a second press while one runs does nothing), then shows the profile and toasts the bands
  and the error before and after, as a measurement from a file does; silence, a missing output and
  any other failure are told in words apart (`missed_the_room`)
  (`a_room_measured_in_the_pane_is_fitted_kept_and_shown_as_the_profile_it_became`,
  `a_room_that_was_not_heard_leaves_nothing_kept_and_says_so`,
  `a_sweep_stopped_part_way_keeps_nothing_and_says_it_was_stopped`,
  `a_calibration_chosen_is_taken_off_what_was_heard_and_a_name_given_is_the_profile_kept`,
  `a_calibration_that_will_not_read_leaves_the_sweep_unplayed_and_nothing_kept`,
  `measure_in_the_equaliser_pane_plays_the_sweep_and_keeps_the_fit_as_a_profile`,
  `measure_in_the_equaliser_pane_keeps_the_name_typed_takes_off_the_calibration_chosen_and_lets_it_go`). The phones
  measure through the same sweep and deconvolution (below).
- **The shared pieces live below the two front ends.** `Sweep::for_measuring(rate)` (20 Hz to 20 kHz
  over `MEASURED_FOR`, 8 s), `Sweep::heard_for` (the sweep plus `HEARD_AFTER_THE_SWEEP`, 2 s) and the
  constants are `resonate-dsp`'s, and `Response::from_levels` (a `Level`'s hertz and decibels as a
  response, `NoMeasuredPoints` for none) and `Response::calibrated_by` (a microphone's response taken
  off point by point) are `resonate-eq`'s, so the binary's `measuring::measured_by` and `calibrated`
  and the boundary's `fit_recorded_sweep` call the same code
  (`levels_become_a_response_and_a_calibration_is_taken_off_point_by_point`).
- **The phones measure through a sweep of their own beside the engine** (`resonate-mobile/src/measurement.rs`,
  `mobile.md`): `measurement_rate_for(output_rate)` (the output's rate where it is 44.1 or 48 kHz, else
  48 kHz), `measurement_sweep(rate)` (the sweep's samples, the `rate` and `recorded_frames`, how many
  frames to record to hear its 2 s tail) and `fit_recorded_sweep(recorded, rate, calibration, target,
  name)`, which deconvolves the mono recording with `Sweep::heard_as`, takes the optional calibration
  file (read as any measurement, `MeasuredFile::Calibration`) off the response and fits it toward the
  target as `fit_measurement` does (`fitted`, one function behind both), answering a `MeasurementFit`
  the phone loads as an import. The sweep never passes the engine, so the equaliser, convolver and
  crossfeed in force do not colour it: Android plays it through a short-lived `AudioTrack` and iOS
  through an `AVAudioPlayerNode` on the engine that also taps the input, each with the player paused
  (`android.md`, `ios.md`). A recording that is silent, shorter than the sweep or at a rate under
  40 kHz is `Error::Sweep`, worded by `failure::of_a_sweep`
  (`a_sweep_through_a_known_filter_is_fitted_as_the_desktops_measurement_is`: a delayed one-pole
  low-pass read within 0.5 dB at 44.1 and 48 kHz, the fit equal to the desktop's of the same levels;
  `a_microphone_calibration_is_taken_off_what_it_heard`,
  `a_recording_that_heard_nothing_or_too_little_or_a_rate_too_slow_is_told_in_words`).

## AutoEq

- **Index `results/INDEX.md`; matching in `resonate-eq`.** No search endpoint: `Corrections` answers
  the whole catalogue and one device's profile; `search`/`suggest` are pure.
- **A row names one device or is skipped.** `- [<Name>](./<source>/<rig-and-form>/<Name>) by
  <Source>[ on <Rig>]`; the folder's last segment must equal the label (integrity check on untrusted
  text); split on `") by "`, not the first `)`. `DeviceId::new` refuses an empty path, one over
  `PATH_AT_MOST` (256), a leading `/`, a backslash, a control character, any `.`/`..` segment (the
  id reaches a URL and a filename).
- **`suggest` answers nothing rather than a guess.** A Bluetooth sink is named by model alone and
  PipeWire appends words, so requiring every word misses real headphones. Matches outright, or on
  the model minus the maker where the rest is telling (two words outside the graph's added
  vocabulary, one mixing letters and digits, or one of 3+ digits); refuses a description naming a
  socket; prefers the longest name; nothing on a tie between two *different* devices
  (`tests/whole_index.rs` is the probe).
- **Measurers are ranked, fixed in `MEASURERS_BEST_FIRST`**: oratory1990, crinacle, Innerfidelity,
  HypetheSonics, Rtings, Super Review, Filk, DHRME, any other last (case-folded). The rank breaks only
  a tie of match and name length, so `suggest` takes the best-ranked measurer of the one device it
  found and `search` lists a device's rows best-ranked first; within a rank the index order stands
  (`a_device_several_measurers_cover_is_suggested_from_the_best_ranked_whatever_the_index_order`).
  **A listener's measurer outranks the list**: `search_preferring`/`suggest_preferring` take the
  name (case-folded) and rank it first among the rows that tie, `resonate eq --measurer` beside
  `--find` or `--suggest`; a device it did not measure keeps the ranking and `--suggest` says so
  (`a_measurer_the_listener_names_is_taken_before_the_ranking_and_a_device_it_did_not_measure_keeps_the_ranking`).
  `equaliser-measurer` keeps one for every device and `[equaliser-measurer-for]` (`node.name` to
  measurer, like `[equaliser-for]`) one for a single device, which outranks it
  (`Config::equaliser_measurer_for`); the flag outranks both, and the pane's search and suggestion
  read the device in use's (`Measurers::for_sink`, `EqualiserModel::look`/`suggested` take the sink;
  `the_pane_suggests_from_the_measurer_preferred_for_the_device_in_use_before_the_one_for_every_device`).
  `--suggest` reads the chosen sink's, `--find` the `--sink` or configured sink's. The *Measured
  corrections* group chooses both live, a field for every device and one for the device in use
  (`prefer_a_measurer`, `prefer_a_measurer_for`: the global, the model and a search standing are
  changed together, then the file; `a_preferred_measurer_is_chosen_from_the_equaliser_pane_and_worn_at_once`,
  `a_measurer_is_chosen_for_the_device_in_use_alone_and_worn_at_once`)
  (`the_pane_suggests_from_the_measurer_the_settings_prefer_before_the_fixed_ranking`).
- **The whole index is one request, and several devices are fetched in one ask.** The index lists
  every measurer's row of every device, so a search of a model lists them all
  (`search`), and `Corrections::catalogue` is fetched once and held for the run (`Arc`) and in the
  catalog for thirty days. A profile is a file per device, so `Corrected::profiles(&[DeviceId])`
  asks each in turn (the host is paced, one queue) and answers a `Fetched` per device, a refusal
  or a miss of one leaving the rest asked. `resonate eq --fetch-every TEXT` is the front end of it:
  it keeps the correction every measurer made for every device whose name holds the words (at most
  `FOUND_AT_MOST`) as `<label> (<measurer>[ on <rig>])`, so the same headphones from several
  measurers can be set against each other in the ABX. It conflicts with every other action. A
  loopback server proves the one index request, the two measurers listed beside each other, the
  fetched and the missing device told apart, and a User-Agent of `resonate/<version>` alone
  (`the_whole_index_is_asked_for_once_and_lists_every_measurer_of_a_model_beside_the_others`).
- **AutoEq stays the one online source, by decision.** The only keyless public source of
  measurements found with a regular shape is squig.link: `/data/phone_book.json` (brands, phones,
  `file`) and `/data/<file> L.txt` / `R.txt` in REW text, which `read_response` already reads; every
  squig site of the CrinGraph tool shares the layout. It is a convention of a tool, not a published
  API: no terms, no stability promise, and the files carry no rig or compensation, so fitting one
  toward Harman would be a guess about a rig (`A guess is never written`). A listener who downloads
  a file uses `--fit-measurement`. Another source lands as a `Corrections` impl beside `AutoEq`
  once one documents its API and its rig; `Corrected::catalogue` still draws only the first source
  that answers, and would merge then.
- **Cached in catalog and process**: `corrections_index`, `corrections_kept` are tables of `V1` (first
  `CHECK (id = 1)`; `library.md`). Ages are `resonate-online`
  constants (index 30 days, device 90); a device with nothing is kept as a miss. `Remembered` is
  `Fresh` or `Stale`, stale read where asking fails, so a no-network session still searches. No
  catalog: the parsed index is held for the run.
- **A URL is escaped once, in `query.rs`**: `escape_path` = `escape_query` per segment.

## The pane

- **The curve is shaped where drawn; typed numbers stay the exact way in.** Press on empty space
  adds a peaking band; on a handle takes it; drag moves; wheel over a handle turns Q; secondary
  press removes.
- **`curve.rs` is the whole pointer, pure arithmetic** (`Plot` read both ways, `placed`, `nearest`).
  `narrowed` (in `resonate-ui/src/equaliser.rs`) takes one step for a turn too small for the next
  hundredth (touchpad trickle). `handle` lifts a band by the preamp, `placed` subtracts it.
- **The drawn range is frozen while a band is held** (`HeldBand` carries the range at the press),
  else a dragged band moves the scale under a still pointer.
- **The pane is the device in use's**: `PlayerModel::sink_in_use` = the sink the stream is open on,
  else the one the engine would bind (engine `chosen_sink`'s order). A press with nothing bound
  gives it an own curve (the fallback's where the graph names no device) and switches the equaliser
  on; *Add a band* too. *Bound to each device* lists the fallback, every plugged device, then every
  device a binding names that the graph does not offer (`unplugged`), so a stale binding can be
  taken away from the window as from `eq --unbind --for`.
- **Sound follows the pointer; a drag never reshapes the chain.** A move onto a different band tells
  the engine the whole `Equalisation` at most every `DRAG_TOLD_EVERY` (50 ms); the timer the first
  move started tells it where the band got to. `retune` sees one band moved as no change of shape; a
  band dragged through 0 dB keeps its stage. The file write is debounced by `PROFILE_SETTLES`
  (600 ms), never lost to it: `EqualiserModel::saved_on_leaving` writes an unsaved curve at the
  entity's release and the application's quit
  (`a_curve_changed_just_before_the_window_closes_is_kept`).
- **What the engine is told is held in memory, not read back from the file** (a typed cell would
  arrive one edit late behind the debounce). `EqualiserModel::held` = every bound curve behind an
  `Arc`, filled by `gather` when bindings change, rewritten by every edit, so an unchanged curve
  keeps its pointer for `OutputSettings`' fast path. A pending save is written when the pane moves
  to another curve; an import/fetch replacing a shown file drops the unsaved copy; replacing a
  *bound* file raises `untold`, answered with `tell_the_engine`
  (`a_bound_profile_kept_again_over_itself_is_what_the_engine_is_told_next`). **Bindings changed
  from outside are followed**: `resonate eq` writes the file and the player's `Reload` reaches the
  window as `Followed::Bindings`, which `RootView::follow` hands to `EqualiserModel::rebound` (the
  unsaved edit is written first, held curves are read afresh, the shown curve is chosen again for
  the sink in use), so the next edit writes from what the engine plays
  (`bindings_changed_from_outside_are_what_the_pane_shows_and_tells_the_engine_next`).
- **The drawn range follows the curve**: `widest_drawn` = largest magnitude over the `CURVE_COLUMNS`
  (192), rounded out to `LEVEL_MARKED_EVERY_DB` (5), never under `DRAWN_BETWEEN_MILLIBELS` (15 dB);
  `marked_levels` labels only the edges (a mid label sits on the trace); the wash runs to the zero
  line.
- **The response is computed on profile change, not per frame**, keyed on a revision counter beside
  the rate; drawn *with* the preamp; the peak beside it is what the bands reach before it, what
  *Fit* sets from. All at `RootView::curve_rate` (negotiated rate, 48 kHz with none open).
- **One shared `Field` and an `Editing { row, cell }` cursor edit every cell.** Enter commits;
  escape and an outside press cancel; tab (the field losing focus any other way: `field::Blurred`,
  `RootView::figure_left`) takes what was typed, and cancels where the value is refused. A cell opens
  holding the shortest text that reads back as the band's own value (`written_exactly`: at least
  0 decimals of hertz, 2 of dB, 3 of Q), so Enter on an untouched cell changes nothing, and the field
  is loaded with `Field::open_with` so ctrl-z in a cell cannot reach the last cell's text
  (`enter_on_an_equaliser_cell_nothing_was_typed_into_keeps_the_band_exactly`,
  `undo_in_an_equaliser_cell_cannot_reach_the_value_of_the_cell_before`,
  `tab_out_of_an_equaliser_cell_takes_what_was_typed_and_closes_it`); a value outside its newtype's bounds is refused into the
  notice naming the limit. `equaliser::read_typed`: unit in any case (`Hz`, `kHz`, `k`, `dB`); a
  comma grouping threes in hertz (`1,000`) is thousands, any other comma is `read_number`'s decimal
  (`1,5 kHz`).
- **The pane owns names, the engine resolution.** Every edit writes one entry via
  `Setting::EqualiserFor` and sends the whole resolved `Equalisation`. `Curve` = `Kept(name)` |
  `Own(owner)`; `Bindings::bound_to` answers one, so a device falling back to the fallback's own
  curve shows and edits that curve. A forgotten profile unbinds every device naming it.
- **Keyboard**: a band moves through its number (`BAND_CONTEXT`, `app::answering_on_a_band`): arrows
  a semitone or half a dB, shift-up/down a notch of Q, via `equaliser::nudged` and the `put` a drag
  uses.
- **The shown curve can be kept under a name or discarded.** *Keep as a profile* under an own curve
  writes a copy via `Store::keep` under the device's description (`unused_name` adds ` 2`, ` 3`);
  binding stays on the own curve. *Discard* takes two presses (`discarding_the_curve`, lowered by
  `disarm`); `discard_the_curve` first unbinds (via `bind_a_curve`) every device and the fallback
  resolving to that curve, then `EqualiserModel::discard` removes the file via
  `Store::forget`/`forget_own`.
- **Every fetch runs on the background executor, each kind of ask in its own task** (`_imported`,
  `_exported`, `_catalogued`, `_fetched`; `is_looking` answers either of the last two), so an import
  beside a catalogue read cannot drop it
  (`a_catalogue_read_is_not_dropped_by_an_import_started_beside_it`).

## The phones

- **One parametric curve, edited as bands** (`resonate-mobile/src/equaliser.rs`):
  `PlaybackPreferences::equaliser` is an `EqualiserCurve` (name, preamp, bands of
  `EqualiserFilter`, one per `BandKind`, frequency, gain and Q as `f32`). `EqualiserCurve::profile`
  is the one way to a `Profile`, so stage, graph and validation share core's quantisation; the
  engine gets it as the `Equalisation` fallback, every route alike; both phones bind a curve to an
  output themselves, sending the one the route in use calls for (`ios.md`, `android.md`). A change of name alone sends
  nothing (`sounds_unlike`).
- **Bounds are the phone's, refused typed before any command**: at most `MAX_BANDS` (32); 20 Hz to
  20 kHz (`LOWEST_HZ`, `HIGHEST_HZ`); gain and preamp within 24 dB (`GAIN_AT_MOST_DB`,
  `PREAMP_AT_MOST_DB`); Q 0.1 to 40, core's own. `TooManyBands`, `BandFrequency`, `BandGain`,
  `BandQ`, `PreampRange` name the band (counted from one) and the value; NaN fails its range.
  `equaliser_limits` hands the bounds to Swift and Kotlin, which keep none of their own
  (`a_curve_past_its_bounds_is_refused_naming_what_is_wrong`).
- **The drawn curve is the stage's arithmetic**: `response_curve(curve, points, rate)` is
  `Profile::response` with the preamp, at the stream rate (48 kHz where none is given) over core's
  `sweep` (log-spaced 20 Hz to 20 kHz, points held to 2..=2048), so point *i* sits at
  `20 · 1000^(i/(n-1))` Hz (`the_drawn_response_of_one_band_is_the_biquad_the_stage_runs`).
- **The phones hold the room correction they apply, so the fit can read it**: `Room::applied` is
  the `Impulse` last sent as `SetConvolution` by `apply_room_correction(true)` and cleared by
  `apply_room_correction(false)` and `forget_room_correction` (a response kept and not applied is
  not in the chain), and `MobilePlayer::applied(curve)` is `Applied::from(curve.profile())`
  `corrected_by` it, the one input of both phone fits.
- ***Fit the preamp* is the desktop's fit, then the queue's**: `MobilePlayer::fitted_preamp(curve,
  rate)` is `Applied::fitted_preamp` (`Profile::fitted_preamp` where no correction is applied, so
  `peak_db`, the bands' loudest point before the preamp, centres weighed beside the sweep; with one,
  the same sweep and centres over the product of the responses) at the stream rate (48 kHz where
  none is given), held to the phone's `PREAMP_AT_MOST_DB`; a curve that only cuts fits 0 dB. Both
  phones offer it beside the preamp slider and keep no peak arithmetic of their own;
  `fitted_preamp_to_the_queue` follows it (above)
  (`the_fitted_preamp_holds_the_loudest_band_under_full_scale_as_the_desktop_does`,
  `the_fit_a_phone_offers_includes_the_room_correction_it_applies`,
  `the_measured_fit_a_phone_offers_includes_the_room_correction_it_applies`).
- **An import becomes editable bands, and says what it gave up**: `parse_eq_profile` (pasted or
  file text) and `auto_eq_profile` answer `EqualiserImport`: every `on` filter is a
  band (an `OFF` filter is dropped, it never sounded), keeping the speaker slots it names as
  `EqualiserBand::channels`, a `ChannelSet` bitmask (`u8`, default 255 = every channel, 0 refused as
  `BandChannels`) that the editors carry unchanged and the kept text restores through `Channel:` lines
  (`a_band_limited_to_some_channels_is_kept_on_them_and_played_on_them_alone`); a value past the phone's bounds is held
  to it and counted in `clamped`, beside the reader's own `passed_over` (an unnameable channel)/`approximated`/`clamped`.
  A GraphicEQ line arrives as its 48 kHz fit and sets `converted`; editing it lets the target go, as
  `band_mut` does on the desktop, so the per-rate refit is not kept.
- **A measurement of the listener's own is a third way in** (`fit_measurement`, *A measurement of
  your own*): it answers the same `EqualiserImport` inside a `MeasurementFit`, so a phone loads it
  as it loads an AutoEq profile.
- **Kept as EqualizerAPO text**: `equaliser_text` writes a valid curve and `parse_eq_profile` reads
  it back as it was (`a_curve_kept_as_text_reads_back_as_the_same_curve`). Both phones store that
  text under `equaliser-curve` and the name under `equaliser-curve-name`, in the `playback` suite or
  `SharedPreferences`.
- **The named presets are one table both phones read**: `equaliser_presets` answers Flat, Bass
  boost, Bass only, Bass cut, Vocals, Warm, Bright, Treble cut, Rock and Electronic in that order
  (an even ten, so the iPhone's two-column grid has no gap in its last row), each a name,
  a line saying what it does and its curve: Flat the default curve (no bands, no name), the rest ten
  octave bands named for the preset. Boosted presets lower the preamp for headroom (the cuts, Bass cut and
  Treble cut, leave it at 0 dB), except Bass
  only, which raises the three lowest octaves and leaves every other band and the preamp at 0 dB
  (`every_preset_is_a_named_octave_curve_and_only_bass_only_boosts_without_lowering_the_preamp`).
  A phone keeps only each preset's icon, keyed by name. A curve is shown as a preset where its name
  is the preset's, or as Flat where it has no gain anywhere; editing a band or the preamp clears the
  name.
- **Ten octave bands are a layout, not the model**: `ten_band_curve(preamp, gains)` is ten peaking
  bands at `EQUALISER_CENTRES` (31.25 Hz to 16 kHz) with Q √2, a non-finite level flat, the rest
  held to the bounds; `ten_band_gains` answers the gains only for a curve of exactly that layout.
  The presets and the phones' "ten flat bands" start use it
  (`ten_octave_gains_make_ten_peaking_bands_at_the_octave_centres`). With no `equaliser-curve` a
  store reads the flat default, which holds no bands, transparent.
