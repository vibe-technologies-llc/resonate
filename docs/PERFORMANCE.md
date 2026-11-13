# Performance

Cost: work done more often, in more memory or on a thread it should not be on. Format and ordering
are in [ROADMAP.md](ROADMAP.md).

## The window

## Startup and idle

## Android

## Performance and scale
- [L · blocked: gpui] Every frame the visualiser or lyrics pane asks for is a whole-window GPU
  paint: gpui 0.2.2, the latest release, draws the scene whole; damage tracking open upstream (zed
  #62455)

## iOS

## Later: Analysis
- [S] `TruePeakMeter` multiplies out the phase at whole samples (weights of one tap of 1 and the rest 0, so the
  answer is the sample itself) but skipping it saves nothing on a vector unit: the phases are lanes of one
  vector, so the lane is free beside the fractions and dropping it only narrows the array (measured, 60 s of
  stereo at 44.1 kHz, fifteen passes: 29.9 G instructions and 18.2 G cycles as it is; the same width with the
  lane left to the fractions and the centre read apart 32.4 G and 18.6 G; arrays of exactly the three
  fractions 60.3 G and 21.8 G, since LLVM does not vectorise three lanes; every read bit for bit the
  same). Only a stereo pair's six fractions packed into one 8-lane vector would save, on AVX-512 alone (the
  release baseline is x86-64-v3, four lanes), and a mono or odd channel has nothing to pack with
- [M] `workers_for` stayed at half the cores though each study now spreads over the crew: four
  workers keep the same throughput as eight at three quarters of the memory (442 MB against 582 MB
  on sixteen songs), but a worker also waits on recognition, which was not measured (retried on a machine
  at a load of 35, where a throughput comparison between four and eight workers means nothing)
- [M] `Bank`'s rows are read one chunk at a time on its own stage, and its forward pass cannot be parted in
  time, so it and `Metered`, 1 to 1.5 s each over 5 minutes at 44.1 kHz, are what a pass's wall time now
  waits for, 96 kHz most of all (4.5 s for 5 minutes). The overlap of reading with the forward pass is already
  there (`together`: the next slab's forward pass beside the last chunks' settling and reading), and the two
  sides are near even, the settling and reading the longer: a bank's share of 60 s of stereo at 44.1 kHz was
  forward 143 ms, settling 148 ms and reading 143 ms on the one-thread crew, and 221 ms against 280 ms
  (settling and reading, settling across the crew) on the wide one. So the stage runs at the reading side's
  pace, and what is left is to part either side without moving a bit: the reading across tile-aligned ranges
  (a `Reading` of its own per range, primed by the three frames before it, the tiles merged by `max`) and the
  forward pass across edge groups (the 28 edges are independent lanes, each a task of a group's own buffer
  gathered into the planes). Neither was built: the machine these were timed on ran at a load of 35, which
  hides wall time, and both gain only on a crew with idle cores (`analyse`, a lone foreground study), not on a
  library pass whose workers already fill the machine. `Metered`'s tile-at-a-time meter reads more than one
  over the whole song does because a tile's exact maximum is lower than the song's, so what the song's
  skips a tile's must read (a bound of every window by its largest taps cut it by a fifth on a quiet master
  and left a loud one as it was: `analysis.md`, *Loudness*); a read that costs less per window, not a
  better skip, is what is left of it
- [M] `peak_through`, `print` and `excerpt` walk on one thread and the crew does not take them: it fans one
  decode out to independent stages, and each of these has one stateful consumer in series (decode, the room's
  convolver, the equaliser, the meter; decode, Chromaprint; decode, a mono mix), so a crew could only overlap
  the decode with it. That is a fifth of `peak_through` (177 to 205 ms for 47 s of 16/44.1 stereo FLAC, 297 ms
  for 40 s at 96 kHz, the decode alone 40 to 50 ms and 70 ms) and most of `print` (51 to 61 ms), and
  `loudest_through` already runs half the cores' worth of cuts at once, so the cores that overlap would buy
  are the ones the cuts use. A gain wants the chain parted, as a pipeline by stage or by channel, and the
  convolver's two-by-two response ties the channels together

## Later: DSP
- [M] The resampler's Very high is level with SoXR VHQ at 2:1, not ahead (`docs/RESAMPLING.md`: 0.041 % of a
  core against 0.040 % at 96 to 48 kHz, 0.043 % against 0.041 % at 48 to 96 kHz), and pays for it in latency
  (300 ms, against 2.8 ms): the forward transform runs at the input's length and most of its bins are
  dropped. A halving stage at 4:1 and beyond would close part of it; pruning the transform to the bins kept was
  worked through and saves one stage of fourteen; an own FFT was measured at parity
