---
paths:
  - "crates/resonate-eq/**/*.rs"
  - "crates/resonate-analysis/src/headroom.rs"
  - "crates/resonate-core/src/eq.rs"
  - "crates/resonate-dsp/src/eq.rs"
  - "crates/resonate-dsp/src/convolve.rs"
  - "crates/resonate-engine/src/impulse.rs"
  - "crates/resonate-online/src/autoeq.rs"
  - "crates/resonate-ui/src/equaliser.rs"
  - "crates/resonate-ui/src/views/settings/equaliser.rs"
  - "crates/resonate-ui/src/views/settings/curve.rs"
  - "crates/resonate/src/equaliser.rs"
  - "crates/resonate-mobile/src/equaliser.rs"
  - "crates/resonate-mobile/src/room.rs"
  - "ios/Sources/Resonate/RoomCorrection.swift"
  - "android/app/src/main/java/org/resonate/android/RoomCorrectionState.kt"
  - "ios/Sources/Resonate/EqualiserGraph.swift"
---

# The equaliser

Parametric, arbitrary bands, bound per device, AutoEq behind it. Chain: `audio.md`; stage contract:
`realtime.md`.

## The vocabulary

- **Arithmetic is `resonate-core::eq`**: stage, `explain`, `resonate eq` and the pane curve cannot
  disagree on a band; `resonate-dsp` stays a core leaf. `resonate-eq` is I/O only (EqualizerAPO and
  GraphicEQ readers, profile store, AutoEq catalogue, the `Corrections` seam `resonate-online`
  fills).
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

- **The model fits a sine; the music is measured.** `Profile::fitted_preamp` assumes full scale at
  the curve's loudest point, so a bass boost over a song with a quiet low end costs headroom it
  never uses. `resonate_analysis::peak_through(sources, location, span, profile, watch)` decodes the
  cut as a study does (`Walking`, the shape it opened in), runs the real `Equaliser` stage over it
  at the file's rate, on the profile `at_rate` that rate and with its preamp taken off, and returns
  the stage output's true peak (`TruePeakMeter::at`, the guard's own interpolator). The preamp is a
  scalar before the bands, so one decode answers: `Preamp::holding_under_full_scale` of that peak.
  Music at full scale on the boost fits as the model does
  (`music_peaking_at_full_scale_on_the_boost_is_fitted_as_the_curve_alone_is`); a boost where the
  music has no energy fits less (`a_boost_where_the_music_has_no_energy_needs_less_preamp_than_the_curve_alone_says`);
  intersample peaks and a sum of components can fit more than the model.
- **A study keeps what a fit needs, so the desktop fits to the whole catalog.** Every study
  stores `track_studies.band_levels` (`BandLevels`: 29 third-octave bands from 25 Hz, the first
  taking everything under 31.5 Hz and the last everything over 16.1 kHz, each one byte, half-dB
  steps below the loudest band; 255 is a band the music never sounded in, all 255 a silent song)
  and `track_studies.band_peaks` (`BandPeaks`: each band's own true peak, `analysis.md`), both
  measured in the study's one decode. The fit reads only the peaks: `BandGains::of(profile, rate)`
  keeps, for each of a few anchor levels (0, 1 and the profile's loudest gain over each band's span,
  `Profile::peak_db_between`), how far the response lies from that level over each band's reach
  (`Profile::farthest_from_between`: the complex response, so phase counts, over the band widened by
  two bands each way, the extent over which the bank puts 2 % of a tone in a band; band 0 reaches
  down to 1 Hz and the last to Nyquist, and per-channel bands count on the channel they reach
  furthest), and `BandGains::bound(peaks, true_peak)` is the least over the anchors `c` of
  `|c| * true_peak + sum(reach_b(c) * peak_b)`. **It is a bound**: the output is
  `c * x + sum((H - c) * x_b)` and the bank's bands sum to the signal exactly, so no anchor can
  fall below the true output peak but by what lies beyond the reach, and the flat anchor makes music away from the curve fit as
  itself while the boosted anchors make a song whose peaks gather in a boosted band fit as the
  boost takes it (`a_song_whose_peaks_gather_in_a_boosted_band_passes_the_estimate_and_not_the_bound`,
  `the_bound_holds_over_what_the_decode_measures_whatever_the_curve_and_where_the_music_sounds`).
  The old estimate, the true peak times the mean gain weighted by each band's energy, is gone: a
  loud sustained band hid the quiet band where the peak gathered. **What stays loose is the
  price of a bound**: bands are summed as if their peaks fell together, so broadband music under a
  curve that moves many bands fits deeper than the decode would, and a narrow boost beside the
  music costs its whole reach; the bank leaves under 2 % of a tone beyond the reach, which the
  bound does not count. `Loudest::weigh` takes the peaks.
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
  none of it runs on the audio thread. Room correction and the volume stage are not in the run (the
  convolver is not part of `Equaliser`); `Impulse::with_headroom` holds the response's loudest gain
  to unity, so its magnitude never passes 0 dB, but the run does not model its shape. The decode
  (`peak_through`) is exact for the stage it runs; the kept figures bound it.
- **A study kept before the bands were measured is measured alone, never studied again.**
  `to_study` lists a current study whose `band_levels` or `band_peaks` is missing or of another
  length as `unmeasured`; the pool decodes it once through `resonate_analysis::band_figures` (the
  study's own transform and bank, so the same figures) and updates those two columns, leaving the
  print, the recognition, the verdict and the `studied` stamp where they stood
  (`a_study_kept_without_its_bands_is_measured_alone_and_keeps_everything_else_it_held`,
  `a_study_kept_without_its_peaks_is_measured_alone_and_is_not_fitted_from_until_it_is`).
  A study under an older `JUDGED_UNDER` is studied whole as ever. Until a song is measured the
  fit decodes it if it is queued and passes it over otherwise.
- **The press keeps the model as the first answer.** The desktop's *Fit* sets the model fit at
  once, then `RootView::fit_the_preamp_to_the_queue` measures on the background executor under a
  `Fitting` (a `Watch` stopped when it is dropped, so a second press or the window going replaces
  it; an empty queue still reads the catalog) and `EqualiserModel::fit_the_preamp_to_the_music`
  lands the measured preamp only if the shown bands are the ones measured, toasting *The preamp is
  fitted to the N songs measured*. The phones read the same bands from their own catalog:
  `MobilePlayer::fitted_preamp_to_the_queue(curve)` folds `Catalog::loudest_kept` (every visible
  document whose `studies.band_peaks` is 58 bytes, through `BandGains::of` at the stream rate,
  48 kHz with none open) with a decode of `Catalog::unmeasured(cuts_to_fit)` alone, the queue's
  songs with no kept peaks (a cut, which no study covers, a document not indexed, one studied
  before `band_peaks` existed or changed since: `documents_forget_where_the_peaks_gathered`), and
  answers the preamp in dB (`None` for nothing measured or all of it silent, held to the phone's
  `PREAMP_AT_MOST_DB`), stopping the fit before it through `Services::fitting`; an empty queue
  still reads the catalog. The pass and the analysis pane keep the bands with the study
  (`keep_studied`, `keep_study`). **A study kept before the bands is measured alone, as on the
  desktop**: `enrichment::measure_the_unmeasured` runs first in the pass's study thread, key or none
  (it asks no service and decodes only studies already kept), over `Catalog::studies_without_bands`
  (every visible document whose study lacks the 29-byte `band_levels` or the 58-byte `band_peaks`),
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
- **The phones keep one response file** (`resonate-mobile/src/room.rs`): `keep_room_correction(bytes,
  extension)` refuses an extension outside `AUDIO_EXTENSIONS` (`ImpulseExtension`) or more than
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
- ***Fit the preamp* is the desktop's fit, then the queue's**: `fitted_preamp(curve, rate)` is
  `Profile::fitted_preamp` (so `peak_db`, the bands' loudest point before the preamp, centres
  weighed beside the sweep) at the stream rate (48 kHz where none is given), held to the phone's
  `PREAMP_AT_MOST_DB`; a curve that only cuts fits 0 dB. Both phones offer it beside the preamp
  slider and keep no peak arithmetic of their own; `fitted_preamp_to_the_queue` follows it (above)
  (`the_fitted_preamp_holds_the_loudest_band_under_full_scale_as_the_desktop_does`).
- **An import becomes editable bands, and says what it gave up**: `parse_eq_profile` (pasted or
  file text) and `auto_eq_profile` answer `EqualiserImport`: every `on` filter is a
  band (an `OFF` filter is dropped, it never sounded), keeping the speaker slots it names as
  `EqualiserBand::channels`, a `ChannelSet` bitmask (`u8`, default 255 = every channel, 0 refused as
  `BandChannels`) that the editors carry unchanged and the kept text restores through `Channel:` lines
  (`a_band_limited_to_some_channels_is_kept_on_them_and_played_on_them_alone`); a value past the phone's bounds is held
  to it and counted in `clamped`, beside the reader's own `passed_over` (an unnameable channel)/`approximated`/`clamped`.
  A GraphicEQ line arrives as its 48 kHz fit and sets `converted`; editing it lets the target go, as
  `band_mut` does on the desktop, so the per-rate refit is not kept.
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
