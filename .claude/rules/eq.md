---
paths:
  - "crates/resonate-eq/**/*.rs"
  - "crates/resonate-core/src/eq.rs"
  - "crates/resonate-dsp/src/eq.rs"
  - "crates/resonate-online/src/autoeq.rs"
  - "crates/resonate-ui/src/equaliser.rs"
  - "crates/resonate-ui/src/views/settings/equaliser.rs"
  - "crates/resonate-ui/src/views/settings/curve.rs"
  - "crates/resonate/src/equaliser.rs"
---

# The equaliser

A parametric equaliser of arbitrary bands at arbitrary frequencies, bound to a device rather than
to the run, with AutoEq's measurements behind it. `audio.md` has the chain it sits in and
`realtime.md` the contract its stage keeps.

## The vocabulary

- **The arithmetic lives in `resonate-core::eq`, for the reason `AppliedGain` does.** The DSP
  stage, `resonate explain`, `resonate eq` and the pane's curve must not disagree about what a band
  is, and `resonate-dsp` stays a leaf on core — `dependencies.md` says losing that grows the
  resampler's test cycle a full link. `resonate-eq` is the twelfth crate and holds only what has
  I/O; it mirrors `resonate-lyrics` and its guard is the same shape: `cargo tree -p resonate-eq`
  stays free of gpui, the engine, the library and `ureq`.
- **Every field of a `Band` is a quantised integer in a newtype, and that is load-bearing three
  times over.** `Frequency` is centihertz, `BandGain` and `Preamp` are millibels, `Q` is
  milli-units. `OutputSettings` derives `Eq` and `publish_settings` compares it every 16 ms to
  decide whether to republish, so a float band would cost that derive — and the day someone
  changes one to an `f32`, `Profile` loses `Eq`, `Equalisation` loses it, `OutputSettings` loses
  it and the workspace stops building, which is the structural enforcement `errors.md` asks for.
  The text format writes integer hertz, one decimal of gain and two of Q, so the quantisation is
  what makes the round-trip exact rather than careful. And `Ord` on an integer is exact where it
  would be a lie on a float.
- **A band says which channels it shapes, and a file that sets each channel apart is read that
  way.** `Band::channels` is a `ChannelSet` — a bit per channel for the first eight, `EVERY` by
  default and for anything wider — and `Band::design_for` answers `Biquad::IDENTITY` for a channel
  the band does not reach. The equaliser designs its sections per channel into the layout its lane
  groups read, each lane of a section carrying its own channel's coefficients, so a left and a
  right filter set run side by side at the cost the shared one had: 0.062 % of a core for ten
  bands against 0.061 %. The EqualizerAPO reader follows `Channel:` lines — `all`, the names
  `L R C SUB RL RR SL SR` and the numbers from 1 — and the writer emits one wherever the next
  band's set differs, so a profile round-trips; a channel it cannot name passes the lines under it
  over rather than widening them to every channel, and so does a `Preamp:` meant for some channels
  and not others, because a preamp here is one for all of them.
- **`Band::new` normalises.** A kind that reads no gain is never built carrying one, so a notch
  cannot hold a value nothing reads and two notches cannot compare unequal over it — which would
  republish `OutputSettings` for a difference that does not exist.
- **`w0` is clamped to π, and that is not a rounding convenience.** A centre above the output's
  Nyquist otherwise wraps `cos` and the band *mirrors down* into the audible range: a 30 kHz shelf
  on a 44.1 kHz stream would land at 14.1 kHz, which sounds like music rather than like a bug. At
  π every kind degenerates to the identity. `Band::applies_at` is the same question asked for
  reporting, and `resonate explain` prints how many bands a rate passed over, the way `audio.md`
  requires a noise-shaping fallback to be visible rather than silent.
- **The magnitude is evaluated directly, not through the cookbook's half-angle form.** The
  half-angle form is exact at DC by construction and was tried for it; measured over the whole
  design grid it is four orders of magnitude *worse* — 5.8e-7 dB against 4.2e-11 dB — because the
  `(b0+b1+b2)²` term and its corrections cancel for a near-unity high-Q section. Tests hold the
  direct form to 1e-9 dB.
- **A measurement's own preamp is the peak this design computes.** AutoEq writes `Preamp:` as the
  negation of the profile's peak response, so one assertion over a captured profile exercises the
  reader, all three band kinds, the coefficient design and the response sweep together. It agrees
  to 0.03 dB on the Sennheiser HD 650.

## The stage

- **Direct form 1, with f64 state, and both halves are load-bearing.** DF1's four words are past
  inputs and past outputs, which mean the same thing after a coefficient change, so a band can be
  retuned under a running stream without a transient; TDF2's two are accumulator residues whose
  meaning is defined by the coefficients that produced them, and substituting new ones is a click.
  That is why a retune swaps the coefficients between two blocks and nothing crossfades the old
  response into the new one: simulated against DF1's carried history, a 10 ms crossfade left a
  ±9 dB swing of a 60 Hz band under music exactly as smooth as it already was, and only halved the
  seam of a ±12 dB swing at the very frequency a tone was playing, which is not worth a second
  cascade run beside the first on every retune. f64 because a 20 Hz shelf at 192 kHz sits its poles within 3e-4 of the unit circle, where f32's
  mantissa leaves a drift floor around -60 dBFS. The sample is widened once on the way in and
  narrowed once on the way out, so a cascade of ten shelves rounds once rather than ten times.
- **`usable` guards the recursive path, and it does two jobs.** An IIR tail decays to the denormal
  range during any long quiet passage, and denormal arithmetic is orders of magnitude slower on
  some machines — a glitch on the engine thread, and `unsafe_code = "forbid"` rules out setting
  FTZ through `std::arch`. A NaN that reaches an IIR never leaves it, so one `is_finite` on the
  same path makes the filter self-healing against a corrupt float. It runs once on the sample
  entering the cascade and once on each section's *output*, and deliberately not on each section's
  input: the input to section *n* is the output of section *n-1*, which has already been through it,
  so guarding it again put a compare-and-select in the middle of a loop-carried dependency for a
  value that could not have been denormal or NaN. Taking it out was bit-identical and measured
  1.39x on the frame-major stage; the stage now costs 0.061 % of a core at ten bands and 0.18 % at
  `MAX_BANDS` natively, 0.081 % and 0.25 % on an `x86-64` baseline build, stereo at 48 kHz. The
  guard reads the magnitude once rather than twice, `is_finite` being `abs() < INFINITY` and the
  denormal test `abs() > DENORMAL_FLOOR`. Frame-major, the native build was the slower of the two:
  under AVX-512 LLVM turns the infinity test into a mask on the serial path, where SSE2 and AVX2
  branch on it.
- **The stage runs section-major over a widened block, four sections in flight at once.**
  `process` widens a piece of the block into `Widened` once — `usable(f64::from(sample) * preamp)`
  — runs each section over the whole piece with its history held in locals, and narrows it once;
  `prepare` sizes the block for `max_frames_in`, and a longer block is taken in pieces. Frame-major,
  a sample went through every section before the next frame could start, each section waiting on
  the one before it, so a frame cost the latency of the whole cascade and at `MAX_BANDS` that chain
  was longer than the out-of-order window could see past. Section-major, what a frame waits on is
  its own section's feedback — one fused multiply-add and the guard, because `biquad` sums the
  feed-forward terms and the `a2` term first and takes the previous output in last, so only the
  newest term is on the loop-carried path where it once waited on two subtractions — and
  `SECTIONS_STAGGERED` sections, each a frame behind the one before, put four of those recursions
  in flight: one at a time measured 0.51 % at `MAX_BANDS` natively against the frame-major 1.55 %,
  two 0.28 %, four 0.18 %, and six or eight spilled and ran slower. Every sample goes through
  `biquad` in the frame-major order, so the output is bit-identical, and
  `the_stage_is_bit_identical_to_a_frame_major_cascade` holds it to a plain frame-major cascade
  across every channel grouping, one section to `MAX_BANDS`, a retune, a shrink and a regrow, a
  burst of NaN and infinities, silence, a reset and blocks longer than the prepared maximum.
  The multiply-adds are fused only where the build targets FMA — `fused::multiply_add` — because
  `f64::mul_add` on a baseline `x86-64` build is a call into libm; fused and last, the stage costs
  0.057 % of a core at ten bands and 0.175 % at `MAX_BANDS`, stereo at 48 kHz. The resampler's
  accumulation was fused the same way and measured no faster, because what it waits on is the
  cache, so it is not.
- **A frame's channels run in groups the kernels are specialised for.** One, two, four, six and
  eight are const generics, so a group's history is registers and its lanes are one vector; mono,
  stereo, quad, 5.1 and 7.1 are one group each, and any other count splits into eights and then
  what is left — three is two and one, twelve is eight and four — each group widened into a run of
  the block of its own. A scalar fallback was slower than the frame-major stage at ten bands. The
  history is carried as `Lanes`, one array per word across the group, rather than as the
  `Section`s it is kept in: carried as structs, LLVM's SLP vectoriser packed a quad's words with
  `vpermt2pd` on the recursion. Each section reaches its frame through a `get_mut` of its own,
  which keeps its store in a basic block of its own: reading the four through one window let SLP
  pack the four sections into one vector with a permute on every feedback path, 2.7x slower
  natively.
  The block and every group in it start on a cache line, because frames straddling one made native
  timings swing by 2x from run to run.
- **Transparency is structural, never numerical, and one predicate answers it for the stage and
  for the plan.** `Profile::is_transparent` is an empty band list and a unity preamp. A profile of
  ten bands all at 0 dB is *not* transparent. `ChainBuilder::build` drops a transparent stage, so
  a numerical rule would take the stage out the moment a slider crossed 0 dB and put it back on the
  way past — two changes of shape in a second on one drag, each a chain rebuilt under the stream at
  best and a stream torn down under a resampler. What it costs is that a bands-loaded, all-flat
  profile keeps a stage in the chain and reads `Converted`; what it buys is that dragging a band
  never gaps the music.
- **`MAX_BANDS` of state is allocated in `prepare`,** kept `[group][section][channel]`, which is
  `[section][channel]` for every named layout, so adding a band while it runs allocates nothing,
  and the lanes a shrink vacates are silenced rather than left to thump when the band count grows
  back.
- **`Equaliser::prepare` refuses a spec that is not the rate it was built for.** The stage is
  handed `plan.stream.rate`, so moving its push above the resampler in `build_chain` fails to
  build with a typed error rather than quietly designing every band at the source rate.

## Where it runs, and what it costs

- **After the remix and the resampler, before the gain, and the dither stays last.** A biquad's
  shape is warped by the rate it is designed at: the same +6 dB Q=1.4 band at 16 kHz measures
  +3.09 dB at 14 kHz designed at 44.1 kHz and +4.97 dB at 96 kHz. Designing at the source rate
  would give one profile a different sound per track, so it is designed at the sink rate, which is
  also the rate the pane draws the curve at. Before the gain because the volume slider is the
  listener's last word and should attenuate a boost, and because `GainStage` is where clamping
  lives in this workspace.
- **An equaliser in force makes the plan `Converted`, and the pane says so in as many words.** It
  is a filter; the samples reaching the device are not the file's own. `OutputMode`'s definition
  names it beside a gain stage.
- **The first band and the last one taken away reshape the chain rather than reopening the stream,
  a resampler included.** Switching the equaliser on or off changes the plan's shape, and
  `retune` swaps a chain built from the new plan in under the stream it already holds wherever
  `OutputPlan::becomes_on_the_same_stream` says it may — `audio.md` has how. Under a resampler the
  running resampler is carried into the new chain, its history with it, rather than rebuilt.
  An equalised plan always carries a gain stage, so neither swap steps the level, and the stage
  crossfades itself in and out: `Easing` is what the engine tells it. A stage *entering* a stream
  that is playing starts wholly dry and blends toward its own output over `EASED_OVER`, 40 ms,
  its biquads running on the real signal from the first frame, so their silent history and a
  preamp's sudden cut are faded in under the dry signal rather than heard as a step; one
  *leaving* blends back to the dry signal the same way, the wanted plan held in
  `Output::settles_into` until it is there, and the chain without it is swapped in only then. A
  profile asked for again while it is leaving is *returning*, which turns the blend round from
  wherever it stands. A settled stage never blends — the weight is exactly one or exactly zero
  away from a fade, so the output is the wet or the dry signal bit for bit — and a paused
  transport swaps at once, having nothing running to click against.
  `switching_the_equaliser_on_and_off_mid_track_glides_rather_than_steps` is the claim: a steady
  level halved by a −6 dB preamp and given back steps by nothing a sample, where the swap alone
  stepped by half the level.
- **A DoP-packed stream refuses it outright.** `untouched` writes `equalisation: None` as a
  literal and `dop_survives` gates on `eq_config`, so there is no reachable path from a marked
  source to a plan with a stage in it. The cross product that proves this now counts the marked
  plans it reached and refuses to pass on zero, because its loop opens with a `continue` and a new
  axis would otherwise prove nothing while looking thorough.
- **A default build is flat and off**, and a whole-plan equality test holds it: a rich
  `Equalisation` with `enabled: false` produces byte-for-byte the plan `EngineConfig::default()`
  does. An enabled equaliser over an empty profile is bit-perfect too, because `eq_config` filters
  on `is_transparent` whatever `enabled` says.
- **The plan and the chain are two judges of transparency, and a test holds them to one answer.**
  Where they disagreed the consequence was not inefficiency: `delivery()` asked the decoder for
  f32, the chain came out empty, `Engine::fill` took the transparent branch and the ring copied raw
  f32 bytes at the sink's integer stride — full-scale noise into the DAC, which a ReplayGain boost
  capped at unity once reached. `Engine::fill` and `Engine::flush` now read the plan for their
  branch, the same reading `delivery()` is made from, so a disagreement costs a format conversion
  rather than noise; the test stays, because a chain that drops a stage the plan asked for leaves
  the plan's `OutputMode` naming work that never ran.
- **The preamp is a scalar at the head of the stage, applied before the bands, because that is
  what the text says it is.** Nothing else guards a boost: `audio.md` says clip prevention
  attenuates rather than limits, and a preamp *is* the attenuation. *Fit* sets it from the
  profile's own peak; a listener who overrides it meets the existing clamps, which is their doing
  and is visible in `resonate explain`.

## The binding

- **A correction is measured for one pair of headphones, so it is bound to a device.**
  `Equalisation` is a `BTreeMap` from `NodeName` to a profile with a fallback — a map rather than
  a list because a list can hold one name twice and because `OutputSettings` is compared sixty
  times a second and a list makes equality order-dependent. Both the map's values and the bundle
  itself are behind `Arc`, so a publish is a refcount bump and the comparison takes the pointer
  fast path.
- **The engine resolves it, not the pane.** `select_sink` falls back to the system default device,
  so only the engine knows which sink a track will open on; `Output::bound` records the name of the
  `SinkInfo` that was actually opened, and `plan_for` takes it. Resolving from `EngineConfig::sink`
  would apply the wrong curve whenever the configured device is absent or unset. `eq_config` is the
  one place a name becomes a profile, mirroring `gain_config`.
- **Switching devices switches profiles for free**, because `SetSink` already rebinds and
  `Output::open` re-resolves. The system default changing under a playing stream does not
  re-resolve; the new device's profile takes effect at the next stream open, which is what
  everything else about the default sink already does.
- **`config.toml` carries a boolean, a binding and a table, and the table holds devices alone.**
  `equaliser` is on or off; `equaliser-profile` is what every device that names none of its own
  falls back to; `[equaliser-for]` maps a `node.name` to what that device is bound to. The fallback
  is a key rather than a reserved entry in the table because a `node.name` is whatever the driver
  says it is — most are dot-qualified, not all are (`auto_null`, `Dummy-Driver`) — so a sink
  literally called `default` used to be unbindable, its entry being read as the fallback instead.
  `a_sink_named_default_is_bound_on_its_own_rather_than_for_the_rest` is the claim now. `Bindings`
  still carries both, filled from either key in whichever order the file writes them, so
  `for_sink` remains the one reading — and it answers *whose* binding it read as well as what it
  says, because a device falling back to the fallback's own curve is handed that curve and not one
  of its own. A dotted node name is written as one quoted key rather than as a path, which is a
  test rather than an assumption, and clearing the last entry takes the table with it.
- **A binding is a kept profile or the device's own curve, and the difference is a type.**
  `resonate_eq::Binding` is `Profile(ProfileName)` or `Own`, and the file writes the first as the
  profile's name and the second as `true` — in either key. That is the lesson the fallback key
  already taught applied once more: a reserved *name* for the own curve would be one a listener
  could give a profile, where no profile can be named `true` without being a string. `false` in
  the fallback key is refused as a value rather than read as nothing, and in the table it is
  passed over with a warning, the way an empty name always was. An own curve needs no name, so
  switching the equaliser on and pressing the curve is the whole of shaping the sound; a device
  that never shaped one holds a flat curve, which `is_transparent` keeps out of the chain.

## The formats

- **Profiles are files, not rows.** `$XDG_DATA_HOME/resonate/equaliser/<name>.txt` in EqualizerAPO
  text, so a profile is hand-editable and any other player reads what is exported. Not `.apo`, not
  `.eq`: `.txt` is what AutoEq and EqualizerAPO both write. A write stages and renames, the
  `settings::File` discipline. It is deliberately not a SQLite table, because `resonate play` and
  `resonate eq` must work against a build with no catalog.
- **An own curve is a profile file too, kept where no name can reach it.** `Store::own` and
  `Store::keep_own` read and write `own/every-other-device.txt` for the fallback and
  `own/device-<node.name>.txt` for a device, in the same text and through the same staged write, so
  a curve shaped in the pane is exported, printed by `resonate eq` and resolved by the engine
  exactly as a kept profile is. The folder has no `.txt` extension, so `names` never lists one as a
  profile. The node name is escaped byte by byte — `[A-Za-z0-9._-]` kept, anything else `%XX`,
  `%` included — and the `device-` prefix is what keeps a device literally called
  `every-other-device` off the fallback's file and `.` or `..` from naming a folder; one too long
  to be a file name is `Error::DeviceNotNameable`.
  `no_device_name_reaches_the_file_another_device_or_the_rest_are_kept_in` is the claim.
- **The command line binds, shapes and forgets an own curve as well as the pane does.**
  `resonate eq --own` binds the device `--for` names — or every other device — to its own curve
  and switches the equaliser on the way `--profile` does; beside `--import` or `--fetch` what is
  read is kept as that curve through `Store::keep_own` rather than as a named profile, and beside
  `--export` it is that curve which is written out, bound or not. `--forget-own` is
  `Store::forget_own` and takes the owner's binding with it only where that binding was the own
  curve, so a device bound to a kept profile keeps it; a device holding neither is
  `Error::NoOwnCurve`. An own curve still outlives its binding on purpose, which is why the
  forgetting is a gesture of its own. `Store::owners` reads the folder back into the owners it was
  written for — a stem is taken only where escaping the name it unescapes to writes the same stem,
  so no hand-made file is read as a device's — and `--list` draws them beside the kept profiles,
  which is how a device gone for good is found to be forgotten.
  `an_own_curve_forgotten_takes_its_file_and_its_binding_and_nothing_else` is the claim.
- **A profile is read as far as it parses and never fails on a line**, the `lrc.rs` and `cue.rs`
  rule. Parameters are read by name rather than by position, `BW Oct` and `S` convert to the Q they
  stand for, a comma decimal reads, and a value past what a band holds is clamped rather than
  dropped — the listener asked for as much as this build can give. Only `LARGEST_PROFILE`,
  `LINES_AT_MOST` and `MAX_BANDS` refuse. `read_number` is exported because the pane's numeric
  cells must agree with the file reader about what a number is.
- **An AutoEq GraphicEQ line is a conversion, and the importer says so.** 127 points carry no bands
  at all, so the curve is fitted onto the 31 ISO third-octave centres at the third-octave Q of
  4.3185 by iterating the bank's response against the target — no linear algebra, `FITTING_PASSES`
  of twelve. Setting each band to the curve's own value at its centre overshoots by about 1.57x,
  because third-octave neighbours sum; the iteration is what corrects that. Measured at 0.035 dB
  over twelve real AutoEq curves and 0.08 dB over four synthetic extremes. Points interpolate
  linearly in log frequency and the endpoints are held rather than extrapolated, because a curve
  ending at 19 kHz must not become a +30 dB band at 20 kHz. A band under `WORTH_A_BAND_MILLIBELS` is dropped
  and the rest refitted, so a flat curve imports as no bands at all.
- **A fit does not travel, so a profile keeps the curve it was fitted to and is fitted again at the
  rate it plays at.** The bilinear warp near a 48 kHz Nyquist is part of what a 48 kHz fit
  compensates for, so the same bands played elsewhere miss: over the fixture curve the worst
  centre was 0.14 dB off at 48 kHz and 0.34 at 44.1, but 2.5 dB at 88.2, 2.8 at 96, 3.7 at 176.4
  and 192 and 4.0 at 384, all of it at 16 kHz — and no one rate fits them all. The fit is
  arithmetic, so it lives in `resonate-core::eq`: a `Target` is the curve's points as quantised
  `TargetPoint`s, `Profile::fitted_to` fits one at `FITTED_AT` and holds it, and
  `Profile::at_rate` answers the fit at another rate — the same `Arc` where there is no curve or
  the rate is `FITTED_AT`. `eq_config` asks it for the stream's rate before it weighs
  transparency, so the stage is handed bands designed for what it plays and `resonate explain`
  prints those. A refit costs about 1.3 ms, and a retune re-plans on every volume step, so a
  `Target` keeps each of the eight `FITS_KEPT_FOR` rates' fits in a `OnceLock` once one is asked
  for — outside `PartialEq` and `Hash`, which weigh the points alone — and holds bands rather than
  a `Profile`, which would be a cycle through its own `Arc`. A preamp moved by hand is carried as
  its distance from the fitted one, so every rate keeps it. The file keeps the curve: a profile
  holding a `Target` is written as its `Preamp:` and the `GraphicEQ:` line EqualizerAPO itself
  reads, and read back into the same `Target`, gains trimmed to the millibel so the round trip is
  exact. A `Preamp:` written beside a curve is the one it plays at, the way EqualizerAPO applies
  both; filter lines beside one are passed over. Shaping a band in the pane or pressing one onto
  it — `band_mut`, `push`, `remove` — lets go of the curve, because the bands are then the
  listener's own and the curve no longer says what they are. A GraphicEQ file imported before the
  curve was kept is parametric bands on disc and nothing can recover the curve from them; importing
  it again is how it gains one.
  `a_curve_fitted_again_at_the_rate_it_plays_at_holds_its_shape_at_every_rate` and
  `a_graphic_curve_is_fitted_again_at_the_rate_the_stream_plays_at` are the claims.

## AutoEq

- **The index is `results/INDEX.md` and the matching lives in `resonate-eq`.** `online.md` rule 12
  says a search answers candidates and the rule that takes one belongs to the caller; here it is
  stronger, because AutoEq offers no search endpoint at all — `raw.githubusercontent.com` serves
  files. So `Corrections` answers with the whole catalogue and with one device's profile, and
  `search` and `suggest` are pure functions provable against a three-row catalogue.
- **A row names one device or it is skipped.** Every line is
  `- [<Name>](./<source>/<rig-and-form>/<Name>) by <Source>[ on <Rig>]`, and the folder's last
  segment equals the label for all 8850 of them — asserted on read, which is a cheap integrity
  check on untrusted third-party text. The path is split on `") by "` rather than on the first
  `)`, because a name may carry brackets and 2560 rows do. `DeviceId::new` refuses an empty path,
  one over `PATH_AT_MOST`, a leading `/`, a backslash, a control character and any `.` or `..`
  segment, because the id reaches both a URL and a filename.
- **`suggest` answers nothing rather than a guess.** A first rule — every word of the device's name
  appears in the description — was measured against real descriptions and rejected: a Bluetooth
  sink is named by the model alone and PipeWire appends its own words, so it missed `WH-1000XM5`
  and `HD 600 Analog Stereo`. What landed matches outright, or on the model with the maker dropped
  where what remains is telling — two words that are not the vocabulary the graph adds, or one
  mixing letters and digits, or one of three digits or more — refuses a description naming a
  socket, prefers the longest name, and answers nothing on a genuine tie between two *different*
  devices. The same headphone measured by three people is not a tie. Against the whole index it
  finds thirteen real headphones including `LE-WH-1000XM4` and refuses every DAC, onboard codec
  and GPU audio device put to it; those probes are the test.
- **The index is cached in the catalog and in the process.** `corrections_index` and
  `corrections_kept` are `V1` tables — the schema is edited where it stands, `library.md` — the
  first carrying a `CHECK (id = 1)` so the type says there is one of it. Thirty days for the index
  and ninety for a device, because AutoEq measures in weeks and re-measures almost never, and a
  device it has nothing for is kept as a miss. An answer past its age is a reason to ask again and
  not a reason to forget: `Remembered` says whether what was kept is `Fresh` or `Stale`, and
  `settled` reads a fresh one as it is, asks again for a stale one and reads the stale one where
  the asking fails — so a session with no network still searches and fetches what the catalog
  holds, however old. A build with no catalog keeps nothing across a restart but still holds the
  parsed index for the run, which `lrclib` has no need of and a search over 8850 rows a keystroke
  does.
- **A URL is escaped once, in `query.rs`.** `escape_path` is `escape_query` per segment, because
  the latter escapes `/` too and a device's name carries spaces, ampersands and brackets.

## The pane

- **The curve is shaped where it is drawn, and a typed number is still the exact way in.** A press
  on empty space adds a peaking band there, a press on a handle takes it, dragging moves it —
  sideways for its frequency, up and down for its gain — the wheel over a handle turns its Q, and
  a secondary press takes it out; the row's discard does the same. A number typed into a row is
  exact where a pointer places a band to the step it can mean, so the cells stay, and a correction
  measured to the hundredth is still entered as one. Every handle is painted in the canvas beside
  the trace, filled in the accent where it is the chosen band, drawn faint where its band is
  switched off, and the chosen band's row wears the accent wash; pressing a row's number chooses
  it, and so does typing into one of its cells.
- **`curve.rs` is the whole of the pointer, and it is arithmetic.** `Plot` is the box the canvas
  painted — its bounds, the range drawn and the preamp — and it reads a pointer both ways:
  `hertz_at` is `across_at` read backwards, `decibels_at` is the drawn level read backwards, and
  `placed` turns a point into a `Placed` frequency and gain on a step a pointer can mean — three
  significant figures of hertz but never finer than a whole one, a tenth of a decibel — clamped to
  20 Hz–20 kHz and to what a `BandGain` holds. `nearest` is the hit test: the closest handle within
  `HANDLE_REACH`, the one drawn on top winning a tie. `narrowed` turns a Q by a sixth of an octave
  a notch on hundredths, and a turn too small to reach the next hundredth still takes one step, so
  a touchpad's trickle of small deltas is not rounded away at a wide Q. None of it needs a window,
  which is why the unit tests are the proof: nothing here can drive a pointer at the compositor.
- **A handle rides on the curve, so the preamp is in its height and taken back off a press.** The
  curve is drawn with the preamp in it, so a handle drawn at its band's own gain floated a whole
  preamp away from the bump it made. `handle` lifts a band by the preamp and `placed` subtracts it,
  so what the pointer reads is the band's gain rather than the level it is drawn at. A kind that
  reads no gain is drawn on the lifted zero line, and dragging it moves its frequency alone,
  because `moved` goes through `Band::new`, which normalises.
- **The drawn range is frozen while a band is held.** `widest_drawn` follows the curve, so a band
  dragged past the range would widen it, move the scale under the pointer and read the same pixel
  as a larger gain on the next move — a runaway to the newtype's bound while the pointer stood
  still. `HeldBand` carries the range taken at the press, the curve is drawn at it and the pointer
  read against it until the release, and a curve that outgrows it is clamped at the edge until
  then.
- **The drag is followed from the window-wide surface the sliders already use.** The press is the
  curve box's own `on_mouse_down`, which is hit-tested, stops propagation and reads the `Plotted`
  cell the canvas's prepaint writes; the moves and the release come through `drag_surface`, which
  now answers a held band as well as a grabbed rail and lets both go on a release inside the
  window, outside it, or a move with no button held. The wheel is consumed only over a handle, so
  scrolling anywhere else on the curve still scrolls the page.
- **A press with nothing bound gives the device a curve of its own.** The pane shows the default
  device's binding, so a press on a curve that shows nothing binds that device's own curve — the
  fallback's where the graph names no default — and switches the equaliser on, the way
  `resonate eq --profile` switches it on when it binds; *Add a band* takes the same path. The
  binding row offers the own curve beside *nothing* and every kept profile, so a device can be
  moved between the two without losing either.
- **The sound follows the pointer, and a drag never reshapes the chain.** Every move that lands on
  a different band tells the engine the whole `Equalisation` at once, and the engine's `retune`
  finds the same shape — one band moved is not a change of shape, and transparency is structural,
  so a band dragged through 0 dB keeps its stage — and swaps coefficients under the running DF1
  history. Only the first band pressed onto an empty curve reshapes the chain, once, as switching
  the equaliser on always did. The file write behind an edit is debounced by `PROFILE_SETTLES`, so
  a drag writes once, after it stops.
- **What the engine is told is held in memory, not read back from the file.** `equalisation()` used
  to read every bound profile off disk, which during a drag would have been a read per move and
  which, behind the debounce, handed the engine the file as it stood *before* the edit — a typed
  cell reached the engine one edit late. `EqualiserModel::held` is every bound curve behind an
  `Arc`, filled once by `gather` when the bindings change and rewritten in place by every edit, so
  a publish is the cache and an unchanged curve keeps its pointer for `OutputSettings`' fast path.
  A save still pending when the pane moves to another curve is written there and then, rather
  than being cancelled with the task that was waiting on it; an import or a fetch that replaces a
  file the pane is showing drops the unsaved copy rather than writing it back over the new one.
- **The wash runs to the zero line, not to the bottom of the box.** The bitrate graph washes to the
  floor because a bitrate is unsigned; an EQ curve is signed, and washing a cut to the floor draws
  it as a boost.
- **The drawn range follows the curve and the axes say what it is.** `widest_drawn` takes the
  largest magnitude the 192 columns reach, rounds it out to `LEVEL_MARKED_EVERY_DB` and never goes
  under `DRAWN_BETWEEN_MILLIBELS`, so a flat profile still reads ±15 dB and a band past that is
  drawn rather than pinned at the edge. `marked_levels` writes that figure at the top and the
  bottom of the right edge and nothing in the middle, the zero line already being drawn — a label
  there sits on the trace. `marked_frequencies` puts 100 Hz, 1 kHz and 10 kHz along the bottom
  through `across_at`, which is `sweep`'s own placement read backwards: the fraction of the width
  is `log10(hz / RESPONSE_FROM_HZ)` over the decades between the two ends. Both are ordinary
  absolutely-positioned children over the canvas rather than painted text, which is why the box is
  `relative`.
- **The response is computed when the profile changes, not per frame**, keyed on a revision counter
  beside the rate — the `PlayerModel::condensed` rule, and for the same arithmetic: 192 points of
  up to 32 biquads at 60 Hz. The curve is drawn *with* the preamp in it, because that is what
  happens to the signal; the peak beside it is what the bands reach before the preamp, which is
  what *Fit* sets it from.
- **One shared `Field` and an `Editing { row, cell }` cursor edit every cell.** Ten bands times
  three numeric cells would be thirty entities where `RootView` holds three. A press opens the
  field in place, enter commits, escape and a press outside cancel, and a value outside its
  newtype's bounds is refused into the pane's notice naming the limit rather than written.
- **The pane owns the names and the engine owns the resolution.** `Bindings` reaches the window the
  way `Online` does, because only the pane needs a profile's *name*; every edit writes one entry
  through `Setting::EqualiserFor` and sends the whole resolved `Equalisation`. `Curve` is what the
  pane shows — `Kept(name)` or `Own(owner)`, the owner `None` for the fallback's — and
  `Bindings::bound_to` answers one, so a device falling back to the fallback's own curve shows and
  edits that curve rather than a new one of its own. A profile forgotten unbinds every device
  naming it, which the store cannot do because it does not know the config.
- **Every fetch runs on the background executor**, because reading an 851 KB index must not block
  a frame, and the file write behind an edit is debounced by `PROFILE_SETTLES` while the engine is
  told at once — the sound should follow the number, a save should not follow a keystroke.
