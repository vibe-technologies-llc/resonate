---
paths:
  - "crates/resonate-eq/**/*.rs"
  - "crates/resonate-core/src/eq.rs"
  - "crates/resonate-dsp/src/eq.rs"
  - "crates/resonate-dsp/src/convolve.rs"
  - "crates/resonate-engine/src/impulse.rs"
  - "crates/resonate-online/src/autoeq.rs"
  - "crates/resonate-ui/src/equaliser.rs"
  - "crates/resonate-ui/src/views/settings/equaliser.rs"
  - "crates/resonate-ui/src/views/settings/curve.rs"
  - "crates/resonate/src/equaliser.rs"
---

# The equaliser

A parametric equaliser of arbitrary bands at arbitrary frequencies, bound to a device rather than
the run, with AutoEq's measurements behind it. `audio.md` has the chain it sits in and
`realtime.md` the contract its stage keeps.

## The vocabulary

- **The arithmetic lives in `resonate-core::eq`, for the reason `AppliedGain` does.** The DSP
  stage, `resonate explain`, `resonate eq` and the pane's curve must not disagree about what a band
  is, and `resonate-dsp` stays a leaf on core (losing that grows the resampler's test cycle a full
  link — `dependencies.md`). `resonate-eq` holds only what has I/O — the EqualizerAPO and GraphicEQ
  readers, the profile store, the AutoEq catalogue with its search and suggestion rules, and the
  `Corrections` seam `resonate-online` fills — on `resonate-core`, `thiserror` and `tracing` alone.
  It mirrors `resonate-lyrics`, with the same guard: `cargo tree -p resonate-eq` stays free of gpui,
  the engine, the library and `ureq`.
- **Every field of a `Band` is a quantised integer in a newtype, load-bearing three times.**
  `Frequency` is centihertz, `BandGain` and `Preamp` millibels, `Q` milli-units. `OutputSettings`
  derives `Eq` and `publish_settings` compares it every 16 ms to decide whether to republish, so a
  float band would cost that derive — change one to `f32` and `Profile`, `Equalisation` and
  `OutputSettings` lose `Eq` and the workspace stops building, the structural enforcement
  `errors.md` asks for. The text format writes integer hertz, one decimal of gain and two of Q, so
  quantisation makes the round-trip exact rather than careful. And `Ord` on an integer is exact where
  on a float it would be a lie.
- **A band says which channels it shapes, and a file setting each channel apart is read that way.**
  `Band::channels` is a `ChannelSet` — a bit per channel for the first eight, `EVERY` by default and
  for anything wider — and `Band::design_for` answers `Biquad::IDENTITY` for a channel the band
  does not reach. The equaliser designs its sections per channel into the layout its lane groups
  read, each lane of a section carrying its own channel's coefficients, so separate left and right
  filter sets cost what the shared one did: 0.062 % of a core for ten bands against 0.061 %. The
  EqualizerAPO reader follows `Channel:` lines — `all`, the names `L R C SUB RL RR SL SR` and the
  numbers from 1 — and the writer emits one wherever the next band's set differs, so a profile
  round-trips; a channel it cannot name passes the lines under it over rather than widening them to
  every channel, as does a `Preamp:` meant for some channels, a preamp here being one for all.
- **A curve is one channel's, and a curve without a channel is the loudest channel's.**
  `Profile::magnitude_db_on` and `response_on` weigh the bands `design_for` that channel, and
  `channels_apart` names the channels some band reaches alone — empty where every band reaches
  every channel, so one curve is the whole story. `magnitude_db`, `response` and `peak_db` weigh
  each channel apart and the rest, and answer the loudest at each point: a left and a right boost
  at one centre are 6 dB where summing them onto one curve made 12, and since the preamp is one for
  all, *Fit the preamp* must hold the loudest channel under full scale. **The pane draws each
  channel apart.** `Profile::responses` answers a `Traced` per curve worth drawing — one
  `TracedOn::EveryChannel` where no band is apart, else a `TracedOn::Channel` for each channel some
  band reaches alone and a `TracedOn::EveryOtherChannel` beside them — and `EqualiserModel::drawn`
  keeps them per revision and rate. The shared curve keeps the accent and its wash, each channel is a
  line in `theme::beside_the_accent`, a key of swatches names them when more than one is drawn
  (`channels_told_apart`, the `L R C SUB RL RR SL SR` order spoken), and the plot is as wide as the
  widest of them. `a_band_shaping_one_channel_is_heard_on_that_channel_alone` is the claim.
- **`Band::new` normalises.** A kind reading no gain is never built carrying one, so a notch cannot
  hold a value nothing reads and two notches cannot compare unequal over it — which would republish
  `OutputSettings` for a difference that does not exist.
- **`w0` is clamped to π, not as a rounding convenience.** A centre above the output's Nyquist
  otherwise wraps `cos` and the band *mirrors down* into the audible range: a 30 kHz shelf on a
  44.1 kHz stream would land at 14.1 kHz, which sounds like music rather than a bug. At π every kind
  degenerates to the identity. `Band::applies_at` asks the same for reporting, and `resonate explain`
  prints how many bands a rate passed over, as `audio.md` requires a noise-shaping fallback to be
  visible.
- **The magnitude is evaluated directly, not through the cookbook's half-angle form.** Half-angle
  is exact at DC by construction and was tried for it; over the whole design grid it is four orders
  of magnitude *worse* — 5.8e-7 dB against 4.2e-11 dB — since the `(b0+b1+b2)²` term and its
  corrections cancel for a near-unity high-Q section. Tests hold the direct form to 1e-9 dB.
- **A measurement's own preamp is the peak this design computes.** AutoEq writes `Preamp:` as the
  negation of the profile's peak response, so one assertion over a captured profile exercises the
  reader, all three band kinds, the design and the sweep together; it agrees to 0.03 dB on the
  Sennheiser HD 650.

## The stage

- **Direct form 1 with f64 state, both halves load-bearing.** DF1's four words are past inputs and
  outputs, meaning the same after a coefficient change, so a band retunes under a running stream
  with no transient; TDF2's two are accumulator residues defined by the coefficients that made them,
  so substituting new ones clicks. Hence a retune swaps coefficients between two blocks and nothing
  crossfades old response into new: simulated against DF1's carried history, a 10 ms crossfade left
  a ±9 dB swing of a 60 Hz band under music exactly as smooth as without, and only halved the seam
  of a ±12 dB swing at the very frequency a tone played — not worth a second cascade on every
  retune. f64 because a 20 Hz shelf at 192 kHz sits its poles within 3e-4 of the unit circle, where
  f32's mantissa leaves a drift floor near −60 dBFS. The sample is widened once in and narrowed
  once out, so ten shelves round once, not ten times.
- **`usable` guards the recursive path, doing two jobs.** An IIR tail decays into the denormal range
  in any long quiet passage, and denormal arithmetic is orders of magnitude slower on some machines
  — a glitch on the engine thread — while `unsafe_code = "forbid"` rules out setting FTZ through
  `std::arch`. A NaN reaching an IIR never leaves, so one `is_finite` on the same path makes the
  filter self-healing against a corrupt float. It runs once on the sample entering the cascade and
  once on each section's *output*, deliberately not each section's input (section *n*'s input is
  *n-1*'s already-guarded output), which put a compare-and-select mid loop-carried dependency for a
  value that could not be denormal or NaN; removing it was bit-identical and 1.39x on the
  frame-major stage, which then cost 0.061 % of a core at ten bands and 0.18 % at `MAX_BANDS`
  natively, 0.081 % and 0.25 % on an `x86-64` baseline build, stereo at 48 kHz. The guard reads the
  magnitude once: `is_finite` is `abs() < INFINITY`, the denormal test `abs() > DENORMAL_FLOOR`.
  Frame-major, the native build was the slower: under AVX-512 LLVM turns the infinity test into a
  mask on the serial path, where SSE2 and AVX2 branch on it.
- **The stage runs section-major over a widened block, four sections in flight.** `process` widens a
  piece of the block into `Widened` once — `usable(f64::from(sample) * preamp)` — runs each section
  over the whole piece with its history in locals, and narrows once; `prepare` sizes the block for
  `max_frames_in`, a longer block taken in pieces. Frame-major, a sample went through every section
  before the next frame could start, each waiting on the one before, so a frame cost the whole
  cascade's latency, at `MAX_BANDS` a chain longer than the out-of-order window could see past.
  Section-major, a frame waits only on its own section's feedback — one fused multiply-add and the
  guard, since `biquad` sums the feed-forward terms and the `a2` term first and takes the previous
  output last, leaving only the newest term on the loop-carried path where it once waited on two
  subtractions — and `SECTIONS_STAGGERED` sections, each a frame behind the one before, keep four
  recursions in flight: one at a time measured 0.51 % at `MAX_BANDS` natively against frame-major
  1.55 %, two 0.28 %, four 0.18 %, and six or eight spilled and ran slower. Every sample goes
  through `biquad` in frame-major order, so the output is bit-identical, and
  `the_stage_is_bit_identical_to_a_frame_major_cascade` holds it to a plain frame-major cascade
  across every channel grouping, one section to `MAX_BANDS`, a retune, a shrink and a regrow, a
  burst of NaN and infinities, silence, a reset and blocks longer than the prepared maximum. The
  multiply-adds are fused only where the build targets FMA (`fused::multiply_add`), `f64::mul_add`
  on a baseline `x86-64` build being a libm call; fused and last, the stage costs 0.057 % of a core
  at ten bands and 0.175 % at `MAX_BANDS`, stereo at 48 kHz. The resampler's accumulation was fused
  likewise and measured no faster — it waits on the cache — so it is not.
- **A frame's channels run in groups the kernels are specialised for.** One, two, four, six and
  eight are const generics, so a group's history is registers and its lanes one vector; mono,
  stereo, quad, 5.1 and 7.1 are one group each, and any other count splits into eights then the
  rest (three is two and one, twelve eight and four), each group widened into its own run of the
  block. A scalar fallback was slower than frame-major at ten bands. The history is carried as
  `Lanes`, one array per word across the group, not as the `Section`s it is kept in: carried as
  structs, LLVM's SLP vectoriser packed a quad's words with `vpermt2pd` on the recursion. Each
  section reaches its frame through a `get_mut` of its own, keeping its store in its own basic
  block: reading the four through one window let SLP pack the four sections into one vector with a
  permute on every feedback path, 2.7x slower natively. The block and every group start on a cache
  line, since frames straddling one made native timings swing 2x run to run.
- **Transparency is structural, never numerical, one predicate answering for stage and plan.**
  `Profile::is_transparent` is an empty band list and a unity preamp; ten bands at 0 dB is *not*
  transparent. `ChainBuilder::build` drops a transparent stage, so a numerical rule would take the
  stage out the moment a slider crossed 0 dB and put it back on the way past — two changes of shape
  in a second on one drag, each a chain rebuilt under the stream at best and a stream torn down
  under a resampler. The cost: a bands-loaded, all-flat profile keeps a stage and reads `Converted`;
  the gain: dragging a band never gaps the music.
- **`MAX_BANDS` of state is allocated in `prepare`**, kept `[group][section][channel]` (which is
  `[section][channel]` for every named layout), so adding a band while it runs allocates nothing,
  and the lanes a shrink vacates are silenced rather than left to thump when the count grows back.
- **`Equaliser::prepare` refuses a spec not at the rate it was built for.** The stage is handed
  `plan.stream.rate`, so moving its push above the resampler in `build_chain` fails to build with a
  typed error (`InputRateMismatch`) rather than quietly designing every band at the source rate.

## Where it runs, and what it costs

- **After the remix and the resampler, before the gain; the dither stays last.** A biquad's shape is
  warped by the rate it is designed at: one +6 dB Q=1.4 band at 16 kHz measures +3.09 dB at 14 kHz
  designed at 44.1 kHz and +4.97 dB at 96 kHz. Designing at the source rate would give one profile
  a different sound per track, so it is designed at the sink rate, the rate the pane draws the curve
  at. Before the gain because the volume slider is the listener's last word and should attenuate a
  boost, and `GainStage` is where clamping lives.
- **An equaliser in force makes the plan `Converted`, and the pane says so in as many words.** It is
  a filter; the samples reaching the device are not the file's own. `OutputMode`'s definition names
  it beside a gain stage.
- **The first band and the last one removed reshape the chain rather than reopening the stream, a
  resampler included.** Switching the equaliser on or off changes the plan's shape, and `retune`
  swaps a chain built from the new plan in under the stream it holds wherever
  `OutputPlan::becomes_on_the_same_stream` says it may (`audio.md` has how); under a resampler the
  running one is carried into the new chain, history and all, and so is a convolver. An equalised
  plan always carries a gain stage, so neither swap steps the level, and the stage crossfades
  itself: `Easing` is what the engine tells it. A stage *entering* a playing stream starts wholly
  dry and blends toward its own output over `EASED_OVER` (40 ms), its biquads running on the real
  signal from the first frame, so their silent history and a preamp's sudden cut fade in under the
  dry signal; one *leaving*
  blends back to dry the same way, the wanted plan held in `Output::settles_into` until it is there,
  and the chain without it is swapped in only then. A profile asked for again while leaving is
  *returning*, turning the blend round from wherever it stands. A settled stage never blends — the
  weight is exactly one or zero away from a fade, so the output is wet or dry bit for bit — and a
  paused transport swaps at once, with nothing running to click against.
  `switching_the_equaliser_on_and_off_mid_track_glides_rather_than_steps` is the claim: a steady
  level halved by a −6 dB preamp and given back steps by nothing a sample, where the swap alone
  stepped by half the level.
- **A DoP-packed stream refuses it outright.** `untouched` writes `equalisation: None` as a literal
  and `dop_survives` gates on `eq_config`, so no path leads from a marked source to a plan with a
  stage. The cross product proving it counts the marked plans it reached and refuses to pass on
  zero, since its loop opens with a `continue` and a new axis would otherwise prove nothing while
  looking thorough.
- **A default build is flat and off**, held by a whole-plan equality test: a rich `Equalisation`
  with `enabled: false` produces byte-for-byte the plan `EngineConfig::default()` does. An enabled
  equaliser over an empty profile is bit-perfect too, since `eq_config` filters on `is_transparent`
  whatever `enabled` says.
- **The plan and the chain are two judges of transparency, held by a test to one answer.** Where
  they disagreed the consequence was not inefficiency: `delivery()` asked the decoder for f32, the
  chain came out empty, `Engine::fill` took the transparent branch and the ring copied raw f32 bytes
  at the sink's integer stride — full-scale noise into the DAC, once reached by a ReplayGain boost
  capped at unity. `Engine::fill` and `Engine::flush` now branch on the plan, the reading
  `delivery()` is made from, so a disagreement costs a format conversion rather than noise; the test
  stays, since a chain dropping a stage the plan asked for leaves `OutputMode` naming work that
  never ran.
- **The preamp is a scalar at the head of the stage, before the bands, as the text says.** Nothing
  else guards a boost: clip prevention attenuates rather than limits (`audio.md`), and a preamp *is*
  the attenuation. *Fit* sets it from the profile's own peak; a listener overriding it meets the
  existing clamps, their doing and visible in `resonate explain`. **A preamp change glides.**
  `set_equalisation` — *Fit*, a typed value, another profile — moves `Preamping` from the amplitude
  in force toward the new one over `EASED_OVER`, a frame at a time, rather than stepping it within
  a sample and ringing the step through the cascade; the ramp is weighed by frames since it began,
  never accumulated, so it lands on the new amplitude exactly whatever the blocks, and the stage
  says `is_ramping` until it has
  (`a_preamp_change_is_eased_in_rather_than_stepped_within_a_sample`). A steady preamp keeps the
  one-multiply `widen` over the whole block.

## Room correction

- **A measured impulse response is convolved with the stream before the equaliser.** `Impulse` is the
  taps of each channel at the rate measured — a stereo response corrects each channel with its own,
  a mono one every channel alike, and one with fewer channels than the stream is read round again —
  and `Impulse::at` draws it again at the stream's rate through the `VeryHigh` resampler, skipping
  the resampler's delay, scaling by the rate ratio so its gain holds, and keeping each rate drawn so
  a second track there costs nothing. `engine::read_impulse` decodes one from any file the codec
  opens, at most `LONGEST_IMPULSE` (10 s), **with the headroom its loudest boost needs**:
  `Impulse::with_headroom` takes the greatest magnitude any channel's response reaches
  (`loudest_gain`, over a spectrum zero-padded to four points a tap) and, where it is above unity,
  scales every channel by its reciprocal, so no frequency comes out louder than it went in and the
  channels keep their balance. A response written quieter is left as written, never raised. A
  measured correction boosting a dip by 6 dB otherwise clipped at the dither's clamp or rode the
  true-peak guard on every loud passage. `Convolver` is uniformly partitioned overlap-save over
  `rustfft`: each partition's spectrum is taken once, a frequency-domain delay line holds the
  input's, and a block out is the inverse of their summed products — one partition behind, its
  latency and what `latency_frames` says. `partition_frames_at` grows the partition with the rate,
  1 024 frames to 48 kHz and a power of two past it, since a partition's cost per sample falls as it
  widens while its latency stays near a fiftieth of a second; a second's response costs 0.45 % of a
  core at 48 kHz and 2.3 % at 192 kHz, where a fixed partition cost 12 %. A track's end flushes the
  tail. `a_long_response_convolves_as_the_direct_sum_does_and_each_channel_takes_its_own` holds it
  to the direct sum.
- **It is `EngineConfig::convolution` and `Command::SetConvolution`, from the `convolution` key.**
  The plan carries it as `OutputPlan::convolution`, making it `Converted` like a curve, and a change
  rebinds at the position rather than retuning, the stage's latency moving the stream. The binary
  reads the file at start and `resonate explain` names it; the settings pane's *Room correction*
  group picks one with the file picker, reads it on a background thread, sends it and writes the
  key, and *Stop correcting* clears both. One response for every device, not a binding: a room is
  measured once and heard through whatever plays in it.

## The binding

- **A correction is measured for one pair of headphones, so it is bound to a device.**
  `Equalisation` is a `BTreeMap` from `NodeName` to a profile, with a fallback — a map because a list
  can hold one name twice and makes equality order-dependent in an `OutputSettings` compared sixty
  times a second. The values and the bundle are behind `Arc`, so a publish is a refcount bump and
  the comparison takes the pointer fast path.
- **The engine resolves it, not the pane.** `select_sink` falls back to the system default, so only
  the engine knows which sink a track opens on; `Output::bound` records the name of the `SinkInfo`
  actually opened, and `plan_for` takes it. Resolving from `EngineConfig::sink` would apply the
  wrong curve whenever the configured device is absent or unset. `eq_config` is the one place a
  name becomes a profile, mirroring `gain_config`.
- **Switching devices switches profiles for free**: `SetSink` rebinds and `Output::open`
  re-resolves, and a system default changing under a playing stream is followed the same way —
  `follow_the_sink_it_would_choose` rebinds the row at its position on the new device, whose plan
  resolves its own profile (`audio.md`).
- **`config.toml` carries a boolean, a binding and a table, the table holding devices alone.**
  `equaliser` is on or off; `equaliser-profile` is what every device naming none of its own falls
  back to; `[equaliser-for]` maps a `node.name` to its binding. The fallback is a key rather than a
  reserved table entry because a `node.name` is whatever the driver says — most dot-qualified, not
  all (`auto_null`, `Dummy-Driver`) — so a sink literally called `default` was once unbindable, read
  as the fallback (`a_sink_named_default_is_bound_on_its_own_rather_than_for_the_rest`). `Bindings`
  carries both, filled from either key in whichever order the file writes them, so `for_sink` is the
  one reading — answering *whose* binding it read as well as what it says, since a device falling
  back to the fallback's own curve is handed that curve, not one of its own. A dotted node name is
  written as one quoted key rather than a path (a test, not an assumption), and clearing the last
  entry removes the table.
- **A binding is a kept profile or the device's own curve, and the difference is a type.**
  `resonate_eq::Binding` is `Profile(ProfileName)` or `Own`, written as the profile's name or
  `true` in either key — the fallback key's lesson once more: a reserved *name* for the own curve is
  one a listener could give a profile, while no profile can be named `true` without being a string.
  `false` in the fallback key is refused as a value; in the table it is passed over with a warning,
  as an empty name always was. An own curve needs no name, so switching the equaliser on and pressing
  the curve is the whole of shaping the sound; a device that never shaped one holds a flat curve,
  which `is_transparent` keeps out of the chain.

## The formats

- **Profiles are files, not rows**: `$XDG_DATA_HOME/resonate/equaliser/<name>.txt` in
  EqualizerAPO text, hand-editable and readable by any other player. `.txt`, not `.apo` or `.eq`,
  is what AutoEq and EqualizerAPO write. A write stages, syncs the bytes, renames
  and syncs the folder (the `settings::File` discipline), so a crash cannot leave a bound profile
  empty. Not a SQLite table, because `resonate play` and `resonate eq` must work with no
  catalog. A `ProfileName` is at most `NAME_AT_MOST` (96) *bytes*, and `ProfileName::after` cuts a
  file's stem at the last letter those bytes hold, so a name in any script is cut rather than
  falling back to `profile` and two long imports keep two files. `resonate eq --import` and `--fetch` say
  when the name they keep under was kept already and is replaced (`Store::holds`). `Store::names` and
  `Store::owners` walk the whole folder and keep the first `PROFILES_AT_MOST` (256) by name in a
  bounded heap, so which are listed past the limit is the folder's alphabet, not its order on disc.
- **An own curve is a profile file too, where no name can reach it.** `Store::own` and
  `Store::keep_own` read and write `own/every-other-device.txt` for the fallback and
  `own/device-<node.name>.txt` for a device, in the same text through the same staged write, so a
  curve shaped in the pane is exported, printed by `resonate eq` and resolved by the engine as a kept
  profile is. The folder has no `.txt` extension, so `names` never lists one. The node name is
  escaped byte by byte — `[A-Za-z0-9._-]` kept, all else `%XX`, `%` included — and the `device-`
  prefix keeps a device literally called `every-other-device` off the fallback's file and `.` or
  `..` from naming a folder; one too long for a file name is `Error::DeviceNotNameable`.
  `no_device_name_reaches_the_file_another_device_or_the_rest_are_kept_in` is the claim.
- **The switch is one for every device.** `equaliser` is a single key, so `resonate eq --off --for
  <sink>` and `--on --for <sink>` are refused by the grammar rather than switching every device;
  one device is let go with `--unbind`. `--suggest` reads the sink `--sink` or the default names,
  so `--for` is refused beside it too.
- **The command line binds, shapes and forgets an own curve as the pane does.** `resonate eq --own`
  binds the device `--for` names — or every other device — to its own curve and switches the
  equaliser on as `--profile` does; beside `--import` or `--fetch` what is read is kept as that
  curve through `Store::keep_own`, and beside `--export` that curve is written out, bound or not.
  `--forget-own` is `Store::forget_own`, taking the owner's binding with it only where that binding
  was the own curve, so a device bound to a kept profile keeps it; a device holding neither is
  `Error::NoOwnCurve`. An own curve outlives its binding on purpose, hence forgetting is a gesture
  of its own. `Store::owners` reads the folder back into the owners it was written for — a stem is
  taken only where escaping the name it unescapes to writes the same stem, so no hand-made file reads
  as a device's — and `--list` draws them beside the kept profiles, which is how a device gone for
  good is found to forget. A kept profile or own curve too large or too broken to read is a row
  reading `unreadable`, warned of on the log, not the end of the list
  (`a_kept_profile_too_large_to_read_is_a_row_of_the_list_and_not_its_end`).
  `an_own_curve_forgotten_takes_its_file_and_its_binding_and_nothing_else`
  is the claim.
- **A profile is read as far as it parses and never fails on a line** (the `lrc.rs` and `cue.rs`
  rule). Parameters are read by name, not position; `BW Oct` and `S` (slope) convert to the Q they
  stand for, a comma decimal reads, and a value past what a band holds is clamped rather than
  dropped — the listener asked for as much as the build gives. A line starting `#` is a comment,
  neither read nor counted as passed over. Only `LARGEST_PROFILE` and `LINES_AT_MOST` refuse; a
  filter past `MAX_BANDS` is passed over and counted like any other unreadable line, the first 32
  kept. `read_number` is exported so the pane's numeric cells agree
  with the file reader about what a number is.
- **An AutoEq GraphicEQ line is a conversion, and the importer says so.** 127 points carry no bands,
  so the curve is fitted onto the 31 ISO third-octave centres at the third-octave Q (`Q::THIRD_OCTAVE`,
  4.318) by iterating the bank's response against the target — no linear algebra, `FITTING_PASSES`
  of twelve. Setting each band to the curve's value at its centre overshoots ~1.57x, third-octave
  neighbours summing; the iteration corrects it. Measured at 0.035 dB over twelve real AutoEq curves
  and 0.08 dB over four synthetic extremes. Points interpolate linearly in log frequency and the
  endpoints are held, not extrapolated, since a curve ending at 19 kHz must not become a +30 dB band
  at 20 kHz. A band under `WORTH_A_BAND_MILLIBELS` is dropped and the rest refitted, so a flat curve
  imports as no bands.
- **A fit does not travel, so a profile keeps the curve it was fitted to and is refitted at the rate
  it plays at.** The bilinear warp near a 48 kHz Nyquist is part of what a 48 kHz fit compensates,
  so the same bands elsewhere miss: over the fixture curve the worst centre was 0.14 dB off at
  48 kHz and 0.34 at 44.1, but 2.5 dB at 88.2, 2.8 at 96, 3.7 at 176.4 and 192 and 4.0 at 384, all at
  16 kHz — no one rate fits them all. The fit is arithmetic, so in `resonate-core::eq`: a `Target`
  is the curve's points as quantised `TargetPoint`s — sorted, de-duplicated on frequency and, past
  `TARGET_POINTS_AT_MOST` (1 024), thinned to evenly spaced ones with both ends kept, never cut at the
  top (`a_curve_of_more_points_than_a_target_holds_is_thinned_across_its_whole_range`) — `Profile::fitted_to` fits one at `FITTED_AT`
  (48 kHz) and holds it, and `Profile::at_rate` answers the fit at another rate — the same `Arc`
  where there is no curve or the rate is `FITTED_AT`. `eq_config` asks it for the stream's rate
  before weighing transparency, so the stage gets bands designed for what it plays and `resonate
  explain` prints those. A refit costs ~1.3 ms and a retune re-plans on every volume step, so a
  `Target` keeps the fits of the eight `FITS_KEPT_FOR` rates in a `OnceLock` once asked — outside
  `PartialEq` and `Hash`, which weigh the points alone — holding bands rather than a `Profile`, which
  would be a cycle through its own `Arc`. A preamp moved by hand is carried as its distance from the
  fitted one, so every rate keeps it. The file keeps the curve: a profile holding a `Target` is
  written as its `Preamp:` and the `GraphicEQ:` line EqualizerAPO itself reads, and read back into
  the same `Target`, gains trimmed to the millibel so the round trip is exact. A `Preamp:` beside a
  curve is the one it plays at, as EqualizerAPO applies both; filter lines beside one are passed
  over. Shaping a band in the pane or pressing one onto it — `band_mut`, `push`, `remove` — lets go
  of the curve, the bands then being the listener's own. A GraphicEQ file imported before the curve
  was kept is parametric bands on disc with no way back to the curve; importing it again gives it
  one. `a_curve_fitted_again_at_the_rate_it_plays_at_holds_its_shape_at_every_rate` and
  `a_graphic_curve_is_fitted_again_at_the_rate_the_stream_plays_at` are the claims.

## AutoEq

- **The index is `results/INDEX.md`, and the matching lives in `resonate-eq`.** `online.md`'s rule
  that a search answers candidates and the caller takes one is stronger here: AutoEq offers no
  search endpoint at all — `raw.githubusercontent.com` serves files. So `Corrections` answers the
  whole catalogue and one device's profile, and `search` and `suggest` are pure functions provable
  against a three-row catalogue.
- **A row names one device or is skipped.** Every line is
  `- [<Name>](./<source>/<rig-and-form>/<Name>) by <Source>[ on <Rig>]`, and the folder's last
  segment equals the label for all 8850 — asserted on read, a cheap integrity check on untrusted
  text. The path is split on `") by "`, not the first `)`, since a name may carry brackets (2560
  rows do). `DeviceId::new` refuses an empty path, one over `PATH_AT_MOST`, a leading `/`, a
  backslash, a control character and any `.` or `..` segment, the id reaching a URL and a filename.
- **`suggest` answers nothing rather than a guess.** A first rule — every word of the device's name
  in the description — was measured against real descriptions and rejected: a Bluetooth sink is
  named by model alone and PipeWire appends its own words, so it missed `WH-1000XM5` and
  `HD 600 Analog Stereo`. What landed matches outright, or on the model with the maker dropped where
  what remains is telling — two words not the graph's added vocabulary, one mixing letters and
  digits, or one of three digits or more — refuses a description naming a socket, prefers the
  longest name, and answers nothing on a genuine tie between two *different* devices (one headphone
  measured by three people is not a tie). Against the whole index it finds thirteen real headphones
  including `LE-WH-1000XM4` and refuses every DAC, onboard codec and GPU audio device put to it;
  those probes are the test.
- **The index is cached in the catalog and the process.** `corrections_index` and
  `corrections_kept` are `V1` tables (laid down before migrations; a change now is a `MIGRATIONS`
  step, `library.md`), the first with `CHECK (id = 1)` so the type says there is one. Thirty days
  for the index and ninety for a device, AutoEq measuring in weeks and re-measuring almost never,
  and a device it has nothing for is kept as a miss. An answer past its age is a reason to ask
  again, not to forget: `Remembered` says whether it is `Fresh` or `Stale`, and `settled` reads a
  fresh one as is, asks again for a stale one and reads the stale one where asking fails — so a
  session with no network still searches and fetches what the catalog holds, however old. A build
  with no catalog keeps nothing across restarts but holds the parsed index for the run, which
  `lrclib` has no need of and a search over 8850 rows a keystroke does.
- **A URL is escaped once, in `query.rs`.** `escape_path` is `escape_query` per segment, the latter
  escaping `/` too, and a device's name carries spaces, ampersands and brackets.

## The pane

- **The curve is shaped where it is drawn, and a typed number is still the exact way in.** A press
  on empty space adds a peaking band there, a press on a handle takes it, dragging moves it —
  sideways for frequency, up and down for gain — the wheel over a handle turns its Q, and a
  secondary press takes it out (as the row's discard does). A number typed into a row is exact where
  a pointer places a band to the step it can mean, so the cells stay and a correction measured to the
  hundredth is still entered as one. Every handle is painted in the canvas beside the trace, filled
  in the accent for the chosen band and faint where its band is off, and the chosen band's row wears
  the accent wash; pressing a row's number chooses it, as does typing into its cells.
- **`curve.rs` is the whole of the pointer, and it is arithmetic.** `Plot` is the box the canvas
  painted — bounds, drawn range, preamp — read both ways: `hertz_at` is `across_at` backwards,
  `decibels_at` the drawn level backwards, and `placed` turns a point into a `Placed` frequency and
  gain on a step a pointer can mean — three significant figures of hertz, never finer than a whole
  one, a tenth of a decibel — clamped to 20 Hz–20 kHz and to a `BandGain`'s range. `nearest` is the
  hit test: the closest handle within `HANDLE_REACH`, the one drawn on top winning a tie. `narrowed`
  turns a Q by a sixth of an octave a notch (`Q_PER_NOTCH`, 2^(1/6)) on hundredths, and a turn too
  small to reach the next hundredth still takes one step, so a touchpad's trickle is not rounded away
  at a wide Q. None of it needs a window, so the unit tests are the proof: nothing here can drive a
  pointer at the compositor.
- **A handle rides on the curve, so the preamp is in its height and taken back off a press.** The
  curve is drawn with the preamp in it, so a handle at its band's own gain floated a whole preamp
  from its bump. `handle` lifts a band by the preamp and `placed` subtracts it, so the pointer reads
  the band's gain, not the drawn level. A kind reading no gain sits on the lifted zero line, and
  dragging it moves only its frequency, `moved` going through `Band::new`.
- **The drawn range is frozen while a band is held.** `widest_drawn` follows the curve, so a band
  dragged past the range would widen it, move the scale under the pointer and read the same pixel as
  a larger gain next move — a runaway to the newtype's bound with the pointer still. `HeldBand`
  carries the range at the press; the curve is drawn at it and the pointer read against it until
  release, a curve outgrowing it clamped at the edge until then.
- **The drag is followed from the window-wide surface the sliders use.** The press is the curve
  box's own `on_mouse_down`, hit-tested, stopping propagation and reading the `Plotted` cell the
  canvas's prepaint writes; moves and release come through `drag_surface`, which answers a held band
  as well as a grabbed rail and lets both go on a release inside or outside the window or a move with
  no button held. The wheel is consumed only over a handle, so elsewhere on the curve it scrolls the
  page.
- **The pane is the device in use's.** `PlayerModel::sink_in_use` answers the sink the stream is
  open on, else the one the engine would bind — the device chosen under Output, else the default,
  else the first, `chosen_sink`'s order — and the Bands group, a press and AutoEq's suggestion all
  read it. The desktop's default alone bound the speakers and suggested for them while a DAC chosen
  under Output was playing
  (`the_sink_in_use_is_the_one_open_else_the_one_the_engine_would_choose`).
- **A press with nothing bound gives the device a curve of its own.** The pane shows the device in
  use's binding, so a press on a curve showing nothing binds that device's own curve — the
  fallback's where the graph names no device — and switches the equaliser on, as `resonate eq
  --profile` does when it binds; *Add a band* takes the same path. The binding row offers the own
  curve beside *nothing* and every kept profile, so a device moves between the two losing neither.
- **The sound follows the pointer, and a drag never reshapes the chain.** A move landing on a
  different band tells the engine the whole `Equalisation` — at most once every `DRAG_TOLD_EVERY`
  (50 ms): a move inside it is noted in `band_untold`, and the timer the first started tells the
  engine where the band got to when it runs out, so a pointer at 240 Hz costs twenty retunes a
  second, not 240, and where it stopped is always what is heard. The engine's `retune` finds the same
  shape — one band moved is no change of shape, and a band dragged through 0 dB keeps its stage — and
  swaps coefficients under the running DF1 history. Only the first band pressed onto an empty curve
  reshapes the chain, once, as switching the equaliser on always did. The file write behind an edit
  is debounced by `PROFILE_SETTLES`, so a drag writes once, after it stops — and is never lost to
  it: `EqualiserModel::saved_on_leaving`, how the window builds the model, has the entity's release
  and the application's quit write an unsaved curve at once (`save_now`), so a band moved in the
  last 600 ms before the window closes is kept
  (`a_curve_changed_just_before_the_window_closes_is_kept`).
- **What the engine is told is held in memory, not read back from the file.** `equalisation()` used
  to read every bound profile off disk — a read per move during a drag, and behind the debounce the
  file as it stood *before* the edit, so a typed cell reached the engine one edit late.
  `EqualiserModel`'s `held` is every bound curve behind an `Arc`, filled once by `gather` when the
  bindings change and rewritten in place by every edit, so a publish is the cache and an unchanged
  curve keeps its pointer for `OutputSettings`' fast path. A save pending when the pane moves to
  another curve is written then, not cancelled with its waiting task; an import or fetch replacing a
  shown file drops the unsaved copy rather than writing it back over the new one. One replacing a
  *bound* file raises `untold`, which the root view's observer takes with the notice and answers with
  `tell_the_engine`, so the new curve is heard at once rather than after the next edit
  (`a_bound_profile_kept_again_over_itself_is_what_the_engine_is_told_next`).
- **The wash runs to the zero line, not the box's bottom.** The bitrate graph washes to the floor, a
  bitrate being unsigned; an EQ curve is signed, and washing a cut to the floor draws it as a boost.
- **The drawn range follows the curve and the axes say what it is.** `widest_drawn` takes the
  largest magnitude the 192 columns (`CURVE_COLUMNS`) reach, rounds it out to
  `LEVEL_MARKED_EVERY_DB` and never goes under `DRAWN_BETWEEN_MILLIBELS`, so a flat profile reads
  ±15 dB and a band past that is drawn rather than pinned. `marked_levels` writes that figure at the
  top and bottom of the right edge and nothing mid-way (the zero line is drawn; a label there sits
  on the trace). `marked_frequencies` puts 100 Hz, 1 kHz and 10 kHz along the bottom through
  `across_at`, `sweep`'s own placement backwards: the width fraction is `log10(hz / RESPONSE_FROM_HZ)`
  over the decades between the ends. Both are absolutely positioned children over the canvas, not
  painted text, hence the box is `relative`.
- **The response is computed when the profile changes, not per frame**, keyed on a revision counter
  beside the rate — the `PlayerModel::condensed` rule, for 192 points of up to 32 biquads at 60 Hz.
  The curve is drawn *with* the preamp, as the signal gets it; the peak beside it is what the bands
  reach before the preamp, what *Fit* sets it from, and is kept against the same revision and rate
  (`EqualiserModel::peaked`), so a render of the Bands group sweeps nothing. All three are worked at
  one rate, `RootView::curve_rate` — the stream's negotiated rate, 48 kHz with none open — since a
  band near the top reads decibels apart at 48 and 96 kHz and a preamp fitted at the one clips or
  gives away headroom at the other
  (`the_peak_and_the_fitted_preamp_are_worked_at_the_rate_the_curve_is_drawn_at`). `Profile::response` and
  `Profile::peak_db` design each band once per sweep and turn each point's angle into its sines and
  cosines once for every section (`Turned`), where the sweep had designed every band again at each
  of its 256 points.
- **One shared `Field` and an `Editing { row, cell }` cursor edit every cell** — ten bands times
  three numeric cells would be thirty entities where `RootView` holds three. A press opens the field
  in place, enter commits, escape and a press outside cancel, and a value outside its newtype's
  bounds is refused into the pane's notice naming the limit. `equaliser::read_typed` reads a cell as a
  person types it: a unit in any case (`Hz`, `KHz`, `k`, `dB`), `kHz` and `k` scaling by a thousand,
  and a frequency in hertz whose commas group threes (`1,000`, `12,500.5`) read as thousands, where
  any other comma is `read_number`'s decimal one (`12,5`, `1,5 kHz`) — a band typed at *1,000 Hz* sat
  at 1 Hz.
- **The pane owns the names, the engine the resolution.** `Bindings` reaches the window as `Online`
  does, only the pane needing a profile's *name*; every edit writes one entry through
  `Setting::EqualiserFor` and sends the whole resolved `Equalisation`. `Curve` is what the pane
  shows — `Kept(name)` or `Own(owner)`, owner `None` for the fallback's — and `Bindings::bound_to`
  answers one, so a device falling back to the fallback's own curve shows and edits that curve. A
  forgotten profile unbinds every device naming it, which the store cannot do without the config.
- **A band is moved from the keyboard through its number.** The band's list number is a tab stop in
  the pane's ring carrying a second key context, `BAND_CONTEXT`, beside the control's, and
  `app::answering_on_a_band` binds the arrows under it: left and right move the band a semitone —
  `2^(1/12)` of its frequency, held within `Frequency::LOWEST` and `HIGHEST` — up and down half a
  decibel within `BandGain::WIDEST_MILLIBELS`, shift-up and shift-down one notch of Q (the wheel's).
  `equaliser::nudged` is that arithmetic, pure and tested; `EqualiserModel::nudge` chooses the band
  and writes it through the same `put` a drag does, so the engine hears at once and the file after
  `PROFILE_SETTLES`. The curve's handles are not tab stops; the number is the keyboard's way to one,
  and its hint says which keys it answers. Nothing here has been driven by a key, KWin offering this
  shell no synthetic input.
- **The shown curve can be kept under a name or discarded, from the pane.** *Keep as a profile*,
  under an own curve, writes a copy through `Store::keep` under the device's description —
  `unused_name` adds ` 2`, ` 3` where a kept profile already folds to it — and leaves the binding on
  the own curve, the copy being something to bind elsewhere. *Discard*, under any shown curve, takes
  two presses, the second under *Press again to discard*, armed on
  `RootView::discarding_the_curve` and lowered by `disarm` like the pane's other armed presses.
  `discard_the_curve` first unbinds, through `bind_a_curve`, every device and the fallback whose
  binding resolves to that curve — writing the config as any binding change does — then
  `EqualiserModel::discard` removes the file through `Store::forget` or `Store::forget_own`,
  dropping an unsaved copy rather than writing it back.
- **Every fetch runs on the background executor**, since reading an 851 KB index must not block a
  frame; the file write behind an edit is debounced by `PROFILE_SETTLES` while the engine is told at
  once — the sound follows the number, a save does not follow a keystroke. **Each kind of ask holds
  a task of its own** — `_imported`, `_exported`, `_catalogued`, `_fetched` — and the catalogue read
  and the fetch a flag each, `is_looking` answering either: one `_asked` task served all four, so an
  import started while the catalogue was being read dropped the read, left the pane looking for the
  rest of the run and never read the catalogue again
  (`a_catalogue_read_is_not_dropped_by_an_import_started_beside_it`).
