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

- **The arithmetic lives in `resonate-core::eq`**, so the DSP stage, `resonate explain`,
  `resonate eq` and the pane's curve cannot disagree about what a band is, and `resonate-dsp` stays
  a leaf on core (`dependencies.md`). `resonate-eq` holds only what has I/O (the EqualizerAPO and
  GraphicEQ readers, the profile store, the AutoEq catalogue with its search and suggestion rules,
  the `Corrections` seam `resonate-online` fills) on core, `thiserror` and `tracing` alone.
- **Every field of a `Band` is a quantised integer in a newtype**: `Frequency` centihertz,
  `BandGain` and `Preamp` milli-decibels, `Q` milli-units. `OutputSettings` derives `Eq` and
  `publish_settings` compares it every 16 ms to decide whether to republish, so a float field
  would stop the workspace building; the text format's precision makes the round trip exact, and
  `Ord` is exact.
- **A band says which channels it shapes.** `Band::channels` is a `ChannelSet` (`EVERY` by default
  and for anything wider than eight), and `Band::design_for` answers `Biquad::IDENTITY` for a
  channel the band does not reach. The EqualizerAPO reader follows `Channel:` lines and the writer
  emits one wherever the next band's set differs; a channel it cannot name, or a `Preamp:` meant
  for some channels (a preamp here is one for all), passes the lines under it over rather than
  widening them.
- **A curve is one channel's, and a curve without a channel is the loudest channel's.**
  `Profile::magnitude_db_on`/`response_on` weigh one channel; `magnitude_db`, `response` and
  `peak_db` answer the loudest at each point (a left and a right boost at one centre are 6 dB, not
  12), so *Fit the preamp* holds the loudest channel under full scale. `channels_apart` names the
  channels some band reaches alone. **The pane draws each channel apart** through
  `Profile::responses` (`Traced` per `TracedOn`), kept per revision and rate by
  `EqualiserModel::drawn`.
- **`Band::new` normalises.** A kind reading no gain never carries one, so two notches cannot
  compare unequal over a value nothing reads and republish `OutputSettings` for nothing.
- **`w0` is clamped to π.** A centre above the output's Nyquist otherwise wraps `cos` and the band
  mirrors down into the audible range (a 30 kHz shelf on 44.1 kHz would land at 14.1 kHz). At π
  every kind degenerates to the identity. `Band::applies_at` asks the same for reporting, and
  `resonate explain` prints how many bands a rate passed over (`audio.md`).

## The stage

- **Direct form 1 with f64 state, both halves load-bearing.** DF1's words are past inputs and
  outputs, meaning the same after a coefficient change, so a band retunes under a running stream
  with no transient; TDF2's accumulator residues would click. So a retune swaps coefficients
  between two blocks and nothing crossfades old response into new. f64 because a 20 Hz shelf at
  192 kHz sits its poles within 3e-4 of the unit circle, where f32 leaves a drift floor near
  -60 dBFS. A sample is widened once in and narrowed once out.
- **`usable` guards the recursive path.** An IIR tail decays into denormals in a quiet passage
  (slow on some machines, and `unsafe_code = "forbid"` rules out setting FTZ), and a NaN reaching
  an IIR never leaves. It runs once on the sample entering the cascade and once on each section's
  *output*, deliberately not each section's input (that is the previous output, already guarded).
- **The stage runs section-major over a widened block, `SECTIONS_STAGGERED` sections in flight.**
  `process` widens a piece of the block into `Widened`, runs each section over the whole piece with
  its history in locals, and narrows once; `prepare` sizes it for `max_frames_in`. Frame-major, a
  frame cost the whole cascade's latency. Every sample still goes through `biquad` in the same
  order, so the output is bit-identical to a frame-major cascade
  (`the_stage_is_bit_identical_to_a_frame_major_cascade`). Multiply-adds are fused only where the
  build targets FMA (`fused::multiply_add`), `f64::mul_add` on a baseline `x86-64` being a libm
  call.
- **A frame's channels run in groups the kernels are specialised for** (one, two, four, six and eight are const generics; any other count splits into eights then the rest), history carried as `Lanes` and each section reaching its frame through its own `get_mut`, which keep LLVM's SLP vectoriser from packing words with a permute on the feedback path.
- **Transparency is structural, never numerical, one predicate answering for stage and plan.**
  `Profile::is_transparent` is an empty band list and a unity preamp; ten bands at 0 dB is *not*
  transparent. `ChainBuilder::build` drops a transparent stage, so a numerical rule would take the
  stage out and put it back as a slider crossed 0 dB, rebuilding the chain under the stream
  twice in a drag. The cost: an all-flat profile with bands keeps a stage and reads `Converted`.
- **`MAX_BANDS` of state is allocated in `prepare`**, `[group][section][channel]`, so adding a
  band while it runs allocates nothing, and the lanes a shrink vacates are silenced rather than
  left to thump when the count grows back.
- **`Equaliser::prepare` refuses a spec not at the rate it was built for** (`InputRateMismatch`),
  so moving its push above the resampler in `build_chain` fails to build rather than designing
  every band at the source rate.

## Where it runs

- **After the remix and the resampler, before the gain; the dither stays last.** A biquad's shape
  is warped by the rate it is designed at, so it is designed at the sink rate, the rate the pane
  draws at, not the source's. Before the gain because the volume slider is the listener's last word
  and should attenuate a boost.
- **An equaliser in force makes the plan `Converted`**, and the pane says so.
- **The first band and the last one removed reshape the chain rather than reopening the stream,
  a resampler included.** `retune` swaps a chain built from the new plan in wherever
  `OutputPlan::becomes_on_the_same_stream` says it may (`audio.md`), carrying a running resampler
  or convolver into it. An equalised plan always carries a gain stage, so neither swap steps the
  level, and the stage crossfades itself: a stage *entering* a playing stream starts wholly dry and
  blends toward its own output over `EASED_OVER`; one *leaving* blends back to dry, the wanted plan
  held in `Output::settles_into` until it is there; a profile asked for again while leaving
  *returns* from wherever the blend stands. A settled stage never blends (wet or dry bit for bit),
  and a paused transport swaps at once
  (`switching_the_equaliser_on_and_off_mid_track_glides_rather_than_steps`).
- **A DoP-packed stream refuses it outright**: `untouched` writes `equalisation: None` and `dop_survives` gates on `eq_config`.
- **A default build is flat and off**, held by a whole-plan equality test. An enabled equaliser
  over an empty profile is bit-perfect too, since `eq_config` filters on `is_transparent`.
- **The plan and the chain are two judges of transparency, held by a test to one answer.** A
  disagreement once put raw f32 bytes at the sink's integer stride; `Engine::fill` and `flush` now
  branch on the plan, which `delivery()` is made from, so a disagreement costs a conversion rather
  than noise, and the test stays so `OutputMode` never names work that never ran.
- **The preamp is a scalar at the head of the stage, before the bands.** Nothing else guards a
  boost: clip prevention attenuates rather than limits (`audio.md`). *Fit* sets it from the
  profile's peak; a listener's override meets the existing clamps, visible in `resonate explain`.
  **A preamp change glides**: `set_equalisation` moves `Preamping` over `EASED_OVER`, weighed by
  frames since it began (never accumulated, so it lands exactly), and the stage says `is_ramping`
  until it has (`a_preamp_change_is_eased_in_rather_than_stepped_within_a_sample`).

## Room correction

- **A measured impulse response is convolved with the stream before the equaliser.** `Impulse` is
  the taps of each channel at the rate measured (a mono response serves every channel, fewer
  channels than the stream are read round again); `Impulse::at` redraws it at the stream's rate
  through the `VeryHigh` resampler, keeping each rate drawn. `engine::read_impulse` decodes any
  file the codec opens, at most `LONGEST_IMPULSE`, **with the headroom its loudest boost needs**:
  `Impulse::with_headroom` scales every channel by the reciprocal of the greatest magnitude any
  channel reaches where that is above unity, so a correction boosting a dip never clips at the
  dither or rides the true-peak guard; a quieter response is never raised. `Convolver` is uniformly
  partitioned overlap-save over `rustfft`, one partition behind (`latency_frames`);
  `partition_frames_at` grows the partition with the rate so cost stays flat. A track's end
  flushes the tail.
- **It is `EngineConfig::convolution` and `Command::SetConvolution`, from the `convolution` key.**
  `OutputPlan::convolution` makes the plan `Converted`, and a change rebinds at the position
  rather than retuning. The binary reads the file at start and `resonate explain` names it; the
  pane's *Room correction* group picks one, reads it on a background thread, sends it and writes the
  key. The read is `RootView::reading_the_room`, one task: a second pick replaces it and *Stop
  correcting* drops it, so the last gesture wins whichever read lands first. One response for every device, not a binding: a room is measured once.

## The binding

- **A correction is measured for one pair of headphones, so it is bound to a device.**
  `Equalisation` is a `BTreeMap` from `NodeName` to a profile with a fallback (a map, because a list
  can hold one name twice and make equality order-dependent in a value compared sixty times a
  second). Values and bundle are behind `Arc`, so a publish is a refcount bump.
- **The engine resolves it, not the pane.** `select_sink` falls back to the system default, so only
  the engine knows which sink a track opens on: `Output::bound` records the opened `SinkInfo`'s
  name and `plan_for` takes it. `eq_config` is the one place a name becomes a profile, mirroring
  `gain_config`. `SetSink` rebinds, `Output::open` re-resolves, and a default changing under a
  playing stream is followed the same way (`follow_the_sink_it_would_choose`).
- **`config.toml` carries a boolean, a binding and a table, the table holding devices alone.**
  `equaliser` is on or off; `equaliser-profile` is the fallback for every device naming none;
  `[equaliser-for]` maps a `node.name` to its binding. The fallback is a key, not a reserved table
  entry, because a `node.name` is whatever the driver says and a sink called `default` must be
  bindable (`a_sink_named_default_is_bound_on_its_own_rather_than_for_the_rest`). `Bindings::for_sink`
  answers *whose* binding it read as well as what it says. A dotted node name is one quoted key,
  and clearing the last entry removes the table.
- **A binding is a kept profile or the device's own curve, and the difference is a type.**
  `resonate_eq::Binding` is `Profile(ProfileName)` or `Own`, written as the name or `true`, since
  no profile can be named `true` without being a string where a reserved name could be given to one.
  `false` is refused in the fallback key and passed over with a warning in the table. A device that
  never shaped an own curve holds a flat one, which `is_transparent` keeps out of the chain.

## The formats

- **Profiles are files, not rows**: `$XDG_DATA_HOME/resonate/equaliser/<name>.txt` in EqualizerAPO
  text, hand-editable and readable by other players, and not a table because `resonate play` and
  `resonate eq` work with no catalog. A write stages, syncs, renames and syncs the folder (the
  `settings::File` discipline). A `ProfileName` is at most `NAME_AT_MOST` *bytes*, cut by
  `ProfileName::after` at the last letter they hold so two long imports keep two files. `eq
  --import` and `--fetch` say when the name was kept already and is replaced (`Store::holds`).
  `Store::names` and `owners` keep the first `PROFILES_AT_MOST` by name, so what is listed past
  the limit is the alphabet's, not the disc's order.
- **An own curve is a profile file too, where no name can reach it.** `Store::own`/`keep_own` use
  `own/every-other-device.txt` for the fallback and `own/device-<node.name>.txt` for a device, so
  it is exported, printed and resolved as a kept profile is, and `names` never lists one. The node
  name is escaped byte by byte (`[A-Za-z0-9._-]` kept, else `%XX`); the `device-` prefix keeps a
  device called `every-other-device` off the fallback's file, and one too long is
  `Error::DeviceNotNameable`
  (`no_device_name_reaches_the_file_another_device_or_the_rest_are_kept_in`).
- **The switch is one for every device.** `equaliser` is a single key, so `eq --off --for <sink>`
  and `--on --for <sink>` are refused by the grammar; one device is let go with `--unbind`.
  `--suggest` reads the sink `--sink` or the default names, so `--for` is refused beside it.
- **The command line binds, shapes and forgets an own curve as the pane does.** `eq --own` binds
  the device `--for` names (or every other device) to its own curve and switches the equaliser on;
  beside `--import`/`--fetch` what is read is kept as that curve, beside `--export` it is written
  out. `--forget-own` (`Store::forget_own`) takes the owner's binding with it only where that
  binding was the own curve; a device holding neither is `Error::NoOwnCurve`. An own curve outlives
  its binding on purpose. `Store::owners` accepts a stem only where escaping its unescaped name
  writes the same stem, so no hand-made file reads as a device's, and `--list` shows them so a
  device gone for good can be forgotten. A file too large or broken to read is an `unreadable` row,
  not the end of the list.
- **A profile is read as far as it parses and never fails on a line** (the `lrc.rs` and `cue.rs`
  rule). Parameters are read by name; `BW Oct` and `S` convert to the Q they stand for, a comma
  decimal reads, a value past what a band holds is clamped, `#` lines are comments. Only
  `LARGEST_PROFILE` and `LINES_AT_MOST` refuse; a filter past `MAX_BANDS` is passed over and
  counted. `read_number` is exported so the pane's numeric cells agree with the reader. **What a
  read gave up is said, by command line and window alike**: `Reading` and `Kept` carry
  `passed_over` (unsupported filters, a `Device:` or unknown `Channel:` scope) and `approximated`
  (a rolloff word other than second order, read as second order).
- **An AutoEq GraphicEQ line is a conversion, and the importer says so.** Its 127 points carry no
  bands, so the curve is fitted onto the 31 ISO third-octave centres at `Q::THIRD_OCTAVE` by
  iterating the bank's response against the target, `FITTING_PASSES` times, since setting each band
  to the curve's value at its centre overshoots as neighbours sum. Points interpolate linearly in
  log frequency and the endpoints are held, not extrapolated (a curve ending at 19 kHz must not
  become a +30 dB band at 20 kHz). A band under `WORTH_A_BAND_MILLI_DECIBELS` is dropped and the
  rest refitted, so a flat curve imports as no bands.
- **A fit does not travel, so a profile keeps the curve it was fitted to and is refitted at the
  rate it plays at.** The bilinear warp near Nyquist is part of what a 48 kHz fit compensates, so
  the same bands miss by dB at higher rates. The fit is arithmetic, so in `resonate-core::eq`: a
  `Target` is quantised `TargetPoint`s, sorted, de-duplicated and thinned across the whole range
  past `TARGET_POINTS_AT_MOST`; `Profile::fitted_to` fits one at `FITTED_AT` and holds it,
  `Profile::at_rate` answers the fit at another rate (the same `Arc` where there is no curve or the
  rate is `FITTED_AT`). `eq_config` asks it for the stream's rate before weighing transparency, so
  the stage and `explain` see bands designed for what plays. Because a retune re-plans on every
  volume step, a `Target` keeps the fits of the `FITS_KEPT_FOR` rates in a `OnceLock`, outside
  `PartialEq` and `Hash`, holding bands rather than a `Profile` (a cycle through its own `Arc`). A
  preamp moved by hand is carried as its distance from the fitted one. The file keeps the curve: a
  profile holding a `Target` is written as `Preamp:` and the `GraphicEQ:` line EqualizerAPO itself
  reads, gains trimmed to the milli-decibel so the round trip is exact; filter lines beside one are
  passed over. Shaping a band (`band_mut`, `push`, `remove`) lets go of the curve.

## AutoEq

- **The index is `results/INDEX.md`, and the matching lives in `resonate-eq`.** AutoEq offers no
  search endpoint (`raw.githubusercontent.com` serves files), so `Corrections` answers the whole
  catalogue and one device's profile, and `search` and `suggest` are pure functions provable
  against a three-row catalogue.
- **A row names one device or is skipped.** Every line is
  `- [<Name>](./<source>/<rig-and-form>/<Name>) by <Source>[ on <Rig>]`, and the folder's last
  segment equals the label (asserted on read as an integrity check on untrusted text). The path is
  split on `") by "`, not the first `)`, since names carry brackets. `DeviceId::new` refuses an
  empty path, one over `PATH_AT_MOST`, a leading `/`, a backslash, a control character and any `.`
  or `..` segment, the id reaching a URL and a filename.
- **`suggest` answers nothing rather than a guess.** A Bluetooth sink is named by model alone and
  PipeWire appends its own words, so requiring every word of the device name misses real
  headphones. It matches outright, or on the model with the maker dropped where what remains is
  telling (two words not the graph's added vocabulary, one mixing letters and digits, or one of
  three digits or more), refuses a description naming a socket, prefers the longest name, and
  answers nothing on a tie between two *different* devices. `tests/whole_index.rs` is the probe.
- **The index is cached in the catalog and the process.** `corrections_index` and
  `corrections_kept` are `V1` tables (the first `CHECK (id = 1)`); a change now is a `MIGRATIONS`
  step (`library.md`). Index and device ages are constants in `resonate-online`, and a device it
  has nothing for is kept as a miss. An answer past its age is a reason to ask again, not to
  forget: `Remembered` is `Fresh` or `Stale`, and a stale one is read where asking fails, so a
  session with no network still searches what the catalog holds. A build with no catalog holds the
  parsed index for the run.
- **A URL is escaped once, in `query.rs`**: `escape_path` is `escape_query` per segment.

## The pane

- **The curve is shaped where it is drawn, and a typed number is still the exact way in.** A press
  on empty space adds a peaking band, a press on a handle takes it, dragging moves it, the wheel
  over a handle turns its Q, a secondary press removes it. Typed cells stay for exact values.
- **`curve.rs` is the whole of the pointer, and it is arithmetic** (`Plot` read both ways, `placed`, `nearest`, `narrowed`), needing no window, so unit tests are the proof; `narrowed` still takes one step for a turn too small for the next hundredth, so a touchpad's trickle is not rounded away.
- **A handle rides on the curve**: `handle` lifts a band by the preamp and `placed` subtracts it.
- **The drawn range is frozen while a band is held** (`HeldBand` carries the range at the press),
  else a band dragged past the range widens it and moves the scale under the pointer, a runaway
  with the pointer still.
- **The pane is the device in use's.** `PlayerModel::sink_in_use` answers the sink the stream is
  open on, else the one the engine would bind (`chosen_sink`'s order), and the Bands group, a press
  and AutoEq's suggestion all read it.
- **A press with nothing bound gives the device a curve of its own** (the fallback's where the graph
  names no device) and switches the equaliser on; *Add a band* takes the same path. The binding row
  offers the own curve beside *nothing* and every kept profile.
- **The sound follows the pointer, and a drag never reshapes the chain.** A move onto a different
  band tells the engine the whole `Equalisation` at most once every `DRAG_TOLD_EVERY`; the timer
  the first move started tells it where the band got to, so where it stopped is always what is
  heard. `retune` sees one band moved as no change of shape, and a band dragged through 0 dB keeps
  its stage. The file write is debounced by `PROFILE_SETTLES` and never lost to it:
  `EqualiserModel::saved_on_leaving` has the entity's release and the application's quit write an
  unsaved curve at once (`a_curve_changed_just_before_the_window_closes_is_kept`).
- **What the engine is told is held in memory, not read back from the file**, or a typed cell would
  reach it one edit late behind the debounce. `EqualiserModel`'s `held` is every bound curve behind
  an `Arc`, filled by `gather` when bindings change and rewritten by every edit, so an unchanged
  curve keeps its pointer for `OutputSettings`' fast path. A pending save is written when the pane
  moves to another curve; an import or fetch replacing a shown file drops the unsaved copy; one
  replacing a *bound* file raises `untold`, answered with `tell_the_engine`
  (`a_bound_profile_kept_again_over_itself_is_what_the_engine_is_told_next`).
- **The drawn range follows the curve and the axes say what it is.** `widest_drawn` takes the
  largest magnitude over the `CURVE_COLUMNS`, rounds it out to `LEVEL_MARKED_EVERY_DB` and never
  goes under `DRAWN_BETWEEN_MILLIBELS`; `marked_levels` labels only the edges (a label mid-way sits
  on the trace). The wash runs to the zero line, the curve being signed.
- **The response is computed when the profile changes, not per frame**, keyed on a revision counter beside the rate. The curve is drawn *with* the preamp; the peak beside it is what the bands reach before it, what *Fit* sets it from. All are worked at `RootView::curve_rate` (the negotiated rate, 48 kHz with none open), since a band near the top reads dB apart at 48 and 96 kHz.
- **One shared `Field` and an `Editing { row, cell }` cursor edit every cell**, not an entity per
  cell. Enter commits, escape and an outside press cancel, and a value outside its newtype's bounds
  is refused into the notice naming the limit. `equaliser::read_typed` reads a cell as a person types
  it: a unit in any case (`Hz`, `kHz`, `k`, `dB`), and a comma grouping threes in hertz (`1,000`)
  as thousands where any other comma is `read_number`'s decimal one (`1,5 kHz`).
- **The pane owns the names, the engine the resolution.** Every edit writes one entry through
  `Setting::EqualiserFor` and sends the whole resolved `Equalisation`. `Curve` is `Kept(name)` or
  `Own(owner)`, and `Bindings::bound_to` answers one, so a device falling back to the fallback's own
  curve shows and edits that curve. A forgotten profile unbinds every device naming it, which the
  store cannot do without the config.
- **A band is moved from the keyboard through its number** (`BAND_CONTEXT`, `app::answering_on_a_band`): arrows move it a semitone or half a decibel, shift-up/down a notch of Q, through `equaliser::nudged` and the same `put` a drag does.
- **The shown curve can be kept under a name or discarded.** *Keep as a profile* under an own curve
  writes a copy through `Store::keep` under the device's description (`unused_name` adds ` 2`, ` 3`)
  and leaves the binding on the own curve. *Discard* takes two presses (`discarding_the_curve`, lowered
  by `disarm`); `discard_the_curve` first unbinds, through `bind_a_curve`, every device and the
  fallback resolving to that curve, then `EqualiserModel::discard` removes the file through
  `Store::forget` or `forget_own`, dropping an unsaved copy.
- **Every fetch runs on the background executor**, and **each kind of ask holds a task of its own**
  (`_imported`, `_exported`, `_catalogued`, `_fetched`; `is_looking` answers either of the last
  two), so an import started beside a catalogue read cannot drop it
  (`a_catalogue_read_is_not_dropped_by_an_import_started_beside_it`).
