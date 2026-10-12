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
  deepest preamp fits the deepest. **It glides**: `set_equalisation` moves `Preamping` over
  `EASED_OVER`, weighed by frames since it began (lands exactly); `is_ramping` meanwhile
  (`a_preamp_change_is_eased_in_rather_than_stepped_within_a_sample`).

## Room correction

- **A measured impulse response is convolved before the equaliser.** `Impulse`: per-channel taps at
  the measured rate (mono serves all; fewer channels than the stream wrap round); `Impulse::at`
  redraws at the stream rate via the `VeryHigh` resampler, cached per rate, time-aligned
  (`a_response_taken_at_another_rate_keeps_its_peak_where_it_was_in_time`). `Impulse::new` refuses a
  non-finite tap. `engine::read_impulse` decodes any codec-readable file, at most `LONGEST_IMPULSE`
  (10 s), **with headroom**: `Impulse::with_headroom` scales every channel by the reciprocal of the
  greatest magnitude where above unity, so a dip-boosting correction never clips; a quieter response
  is never raised. `Convolver`: uniformly partitioned overlap-save over `rustfft`, one partition
  behind (`latency_frames`); `partition_frames_at` grows it with the rate (flat cost); a track's end
  flushes the tail.
- **`EngineConfig::convolution`, `Command::SetConvolution`, key `convolution`.**
  `OutputPlan::convolution` makes the plan `Converted`; a change rebinds at the position, no retune.
  The binary reads the file at start, `explain` names it; the pane's *Room correction* group reads
  one on a background thread. `RootView::reading_the_room` is one task: a second pick replaces it,
  *Stop correcting* drops it, so the last gesture wins. One response for all devices, no binding.

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
  catalog. A write stages, syncs, renames, syncs the folder (`settings::File` discipline).
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
- **Cached in catalog and process**: `corrections_index`, `corrections_kept` are `V1` tables (first
  `CHECK (id = 1)`); a change is a `MIGRATIONS` step (`library.md`). Ages are `resonate-online`
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
  (`a_bound_profile_kept_again_over_itself_is_what_the_engine_is_told_next`).
- **The drawn range follows the curve**: `widest_drawn` = largest magnitude over the `CURVE_COLUMNS`
  (192), rounded out to `LEVEL_MARKED_EVERY_DB` (5), never under `DRAWN_BETWEEN_MILLIBELS` (15 dB);
  `marked_levels` labels only the edges (a mid label sits on the trace); the wash runs to the zero
  line.
- **The response is computed on profile change, not per frame**, keyed on a revision counter beside
  the rate; drawn *with* the preamp; the peak beside it is what the bands reach before it, what
  *Fit* sets from. All at `RootView::curve_rate` (negotiated rate, 48 kHz with none open).
- **One shared `Field` and an `Editing { row, cell }` cursor edit every cell.** Enter commits;
  escape and an outside press cancel; a value outside its newtype's bounds is refused into the
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
