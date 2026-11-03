# Resonate's resampler against SoXR

How `resonate-dsp`'s resampler compares with libsoxr, the resampler most audio software treats as
the reference, measured with both in one process on the same buffers. Measured on 2026-11-04 with
libsoxr 0.1.3 as Arch ships it, rustc 1.99.1 and `target-cpu=native` on an AMD Ryzen 7 7700, after
the two changes this comparison led to (*What changed in Resonate*). `soxr-comparison/` is the
harness (`.claude/rules/build.md`); the `compare-resamplers` skill reruns it.

## In short

- **At every level that matters, Resonate's filter is at least as clean as SoXR's and keeps more
  of the top octave.** High, the default, rejects aliases and images to −136.6 dB against SoXR
  HQ's −132.0 dB and stays flat to 20.83 kHz (−0.1 dB) where HQ stops at 20.53 kHz; Very high
  reaches −185.8 dB against VHQ's −183.6 dB and stays flat to 21.34 kHz against 20.57 kHz
  (44.1 kHz to 48 kHz).
- **Resonate computes in f64 throughout; SoXR HQ computes in single precision.** HQ's noise stops
  at about 133 dB SNR whatever the filter; High reaches 196.6 dB at 1 kHz and 147.1 dB at its worst
  tone, Very high 246.4 dB and 202.7 dB.
- **Resonate's top levels cost 2.5 to 4 times SoXR's, down from 3 to 22.** High and Very
  high now filter through FFTs in two stages: at 44.1 to 48 kHz Very high takes 0.165 % of a core
  against VHQ's 0.054 % (it took 0.356 %), and at 192 to 48 kHz 0.332 % against 0.084 % (it took
  1.867 %). High takes 0.145 % against HQ's 0.053 %.
- **The price was a little latency.** High and Very high now answer 2.9 to 3.5 ms later than they
  did: Very high's peak leaves 10.75 ms after its impulse at 44.1 to 48 kHz, where VHQ's leaves
  after 8.41 ms; at 48 to 44.1 kHz Very high takes 10.44 ms and VHQ 30.85 ms, since SoXR's block
  depends on the ratio.
- **SoXR's top recipes go further than Resonate's top level.** The 32-bit recipe rejects to
  −207.0 dB, which no Resonate level does, and VHQ steep keeps the passband to 21.75 kHz. Both
  sit far below anything a 24-bit converter resolves (−144 dB).
- **SoXR's passband is deliberately rolled off by up to 0.01 dB under its edge**, so from a
  44.1 kHz source a null against the ideal over 20 Hz–20 kHz bottoms out near −80 dB for HQ and
  every recipe above it; Resonate's High and Very high null to −155 dB and −208 dB there.
- **Resonate's filters ring for longer.** A narrower transition needs a longer kernel: Very high
  rings 5.7 ms either side of an impulse against VHQ's 2.5 ms, with its energy more than 1 ms
  before the peak at −35 dB against VHQ's −47 dB.
- **Fast and Balanced now start their stopband at Nyquist**, as SoXR's recipes do: lengthened to
  19 and 47 taps a side, the fewest that do it at their cutoff and β, their worst leak past
  Nyquist falls from −39 and −36 dB to −63 and −90 dB for 5 % and 17 % more CPU, and their
  −3 dB points rise. Narrowing their cutoffs instead would have cost 4 to 5 dB at 20 kHz.

## What was compared

Eighteen settings at seven rate pairs: 44.1 to 48 kHz (a CD on a 48 kHz device), 48 to 44.1,
96 to 48, 96 to 44.1, 192 to 48, 44.1 to 96 and 48 to 96.

| Setting | Design |
|---|---|
| **Resonate Fast** | Kaiser-windowed sinc, 19 taps a side, cutoff 0.900 of the band edge, β 6.0 (about 63 dB), one polyphase stage |
| **Resonate Balanced** | 47 taps a side, cutoff 0.940, β 9.0 (about 90 dB), one polyphase stage |
| **Resonate High** (default) | 128 taps a side, cutoff 0.965, β 14.0 (about 136 dB), two stages |
| **Resonate Very high** | 320 taps a side, cutoff 0.978, β 19.5 (about 186 dB), two stages |
| **Resonate High, minimum** and **Very high, minimum** | the same kernels folded to minimum phase through the real cepstrum |
| **Resonate Very high, intermediate** | half minimum, half linear phase |
| SoXR QQ | cubic interpolation, no filter |
| SoXR LQ | 16-bit precision, passband to 0.676 of Nyquist, medium rolloff |
| SoXR MQ | 16-bit, passband to 0.916, medium rolloff |
| SoXR 16-bit | 16-bit, passband to 0.916, small rolloff |
| SoXR HQ (its default) | 20-bit, passband to 0.914 |
| SoXR 24-bit | 24-bit, passband to 0.912 |
| SoXR VHQ | 28-bit, passband to 0.911 |
| SoXR 32-bit | 32-bit, passband to 0.911 |
| SoXR VHQ, steep | VHQ with `SOXR_STEEP_FILTER`'s passband, 0.982 |
| SoXR VHQ, intermediate and minimum | VHQ at phase response 25 and 0 |

Resonate's taps are counted at the lower of the two rates. Fast and Balanced tabulate their kernel
for every phase the exact rational ratio needs (160 for 44.1 to 48 kHz) and sum each output frame
over it. High and Very high convolve the same kernel by FFT at twice the input rate, then
interpolate to the output with a short kernel 60 dB quieter than their own stopband
(*What changed in Resonate*). SoXR runs on one thread, with its own defaults otherwise: its
stopband begins at Nyquist at every recipe, and recipes of 20 bits and fewer compute in single
precision. Both take and give interleaved f64, so neither pays for a conversion the other does not.

## How it was measured

Every figure is read from what each resampler made of a signal, never from its design.

- **Tones on an exact grid.** Each tone is 1.2 s at −6 dBFS on a 1/64 Hz grid, so its phase at
  any sample reduces exactly and stays accurate to the f64 floor however far into the signal.
  The first and last 0.3 s, where a filter starts and stops, are thrown away.
- **A least-squares fit at known frequencies.** Over the settled 0.6 s, the output is fitted with
  a sine and cosine at the tone's frequency and, where it lands apart from it, at its first image
  `input rate − f`, folded into the output band. What is left after taking the tone away is
  everything the resampler added: images, aliases, noise and rounding.
- **The sweep.** 64 tones spaced logarithmically from 20 Hz to 0.85 of the band edge, 48 spaced
  evenly from there to 0.999 of it, plus 1 kHz and 20 kHz; going down, a further 24 tones from
  the output's Nyquist to 1.2 of it and 40 from there to the input's Nyquist.
- **Ripple** is the spread of gain over the tones from 20 Hz to 10 kHz; **deviation** is the same
  over 20 Hz to 20 kHz, so it shows a filter rolling off under 20 kHz. **−0.1 dB** and **−3 dB**
  are where gain crosses those levels, interpolated between tones.
- **Worst alias or image** is the loudest of every tone above the output's Nyquist (its whole
  output, since none of it belongs there) and every fitted image, against the tone; **worst
  landing ≤ 20 kHz** is the same over what lands in the audible band.
- **SNR** is the tone against everything else in the output, at 1 kHz and at the worst tone from
  20 Hz to 20 kHz. Above about 250 dB it is f64's own rounding.
- **Null** is 48 tones with random phases, spread logarithmically from 20 Hz to 20 kHz (or to
  10 kHz), against the ideal output computed from the same tones, the output's delay at 1 kHz
  taken out. A minimum or intermediate phase filter cannot null: its phase is not the input's.
- **Delay** is the phase delay at 1 kHz, and its **spread** from 20 Hz to 20 kHz, unwrapped from
  the impulse's peak. A linear-phase filter's is zero, since both libraries align their output.
- **Ringing** is how long the impulse response stays above −120 dB of its peak before and after
  it; **energy over 1 ms before the peak** is the share of the response's energy that arrives
  more than a millisecond early: pre-echo.
- **Streaming latency** feeds one frame at a time and times the impulse's peak leaving after the
  impulse arrives: the least delay each setting can be run at.
- **Throughput** is stereo white noise at −6 dBFS, 4 s of it in blocks of 1 024 frames, the best
  of five runs on an idle machine: the share of one core a second of audio costs. **Construction**
  is the first build of a setting in the process, then the best of nine builds after it.
- The plots draw the response as the sweep read it: the tone's gain under the output's Nyquist,
  and above it the alias or image level at the frequency it came from. Their floor is −220 dB;
  anything lower is drawn on it.

## Results

### Best against best

Resonate's best setting, Very high in linear phase, against SoXR's three strongest: VHQ, the
32-bit recipe and VHQ steep. The best figure in each row is in bold.

| 44.1 kHz to 48 kHz unless named | **Resonate Very high** | SoXR VHQ | SoXR 32-bit | SoXR VHQ, steep |
|---|---:|---:|---:|---:|
| Ripple 20 Hz–10 kHz (dB) | **4.2e-10** | 1.4e-8 | 7.2e-9 | 1.3e-8 |
| Deviation 20 Hz–20 kHz (dB) | **< 0.0001** | 0.0062 | 0.0078 | 0.0017 |
| −0.1 dB (kHz) | 21.34 | 20.57 | 20.58 | **21.75** |
| −3 dB (kHz) | 21.51 | 20.95 | 20.94 | **21.82** |
| Worst alias or image (dB) | −185.8 | −183.6 | **−207.0** | −181.6 |
| Worst landing ≤ 20 kHz (dB) | −210.7 | −201.4 | **−222.0** | −214.6 |
| 96 → 48 kHz: worst alias (dB) | −185.8 | −177.3 | **−216.7** | −192.3 |
| 192 → 48 kHz: worst alias (dB) | −185.8 | −183.9 | **−215.7** | −183.9 |
| SNR at 1 kHz (dB) | **246.4** | 187.0 | 212.3 | 189.9 |
| Worst SNR 20 Hz–20 kHz (dB) | 202.7 | 184.1 | **207.1** | 183.9 |
| Null 20 Hz–20 kHz (dB) | **−208.0** | −79.7 | −77.7 | −91.1 |
| Null 20 Hz–10 kHz (dB) | **−211.2** | −182.4 | −190.7 | −183.0 |
| Ringing either side (ms) | 5.67 | **2.52** | 2.69 | 11.77 |
| Energy over 1 ms before the peak (dB) | −35.3 | **−46.5** | −45.1 | −31.6 |
| Streaming latency (ms) | 10.75 | **8.41** | 18.44 | 24.83 |
| CPU, % of a core | 0.165 | **0.054** | 0.061 | 0.073 |
| 192 → 48 kHz: CPU, % of a core | 0.332 | **0.084** | 0.097 | 0.097 |

Very high is the most transparent in the audible band: the flattest passband, no rolloff under
20 kHz, and a null 20 to 29 dB deeper than any SoXR recipe's to 10 kHz and 117 dB deeper to
20 kHz. SoXR 32-bit rejects aliases and images 11 to 31 dB further; VHQ steep keeps 0.4 kHz more
of the passband; VHQ rings for half as long, answers 2.3 ms sooner here and costs a third of the
CPU.

### 44.1 kHz to 48 kHz

The case nearly every listener with a 48 kHz device hears.

| Setting | −0.1 dB (kHz) | Worst alias or image (dB) | Worst landing ≤ 20 kHz (dB) | Worst SNR (dB) | Null to 10 kHz (dB) | Streaming latency (ms) | CPU (%) |
|---|---:|---:|---:|---:|---:|---:|---:|
| **Resonate High** | 20.83 | −136.6 | −158.3 | 147.1 | −168.8 | 6.26 | 0.145 |
| SoXR HQ | 20.53 | −132.0 | −141.5 | 131.1 | −131.3 | 18.39 | 0.053 |
| **Resonate Very high** | 21.34 | −185.8 | −210.7 | 202.7 | −211.2 | 10.75 | 0.165 |
| SoXR VHQ | 20.57 | −183.6 | −201.4 | 184.1 | −182.4 | 8.41 | 0.054 |
| SoXR 32-bit | 20.58 | −207.0 | −222.0 | 207.1 | −190.7 | 18.44 | 0.061 |
| SoXR VHQ, steep | 21.75 | −181.6 | −214.6 | 183.9 | −183.0 | 24.83 | 0.073 |

![Magnitude response and rejection, 44.1 kHz to 48 kHz](resampling/response-44.1-to-48.svg)

![Top of the passband, 44.1 kHz to 48 kHz](resampling/passband-44.1-to-48.svg)

High and Very high hold their passbands flat almost to their −3 dB points (21.16 and 21.51 kHz),
then fall into their stopbands by the input's Nyquist: Resonate trades a longer kernel for a
narrower transition. SoXR's linear recipes keep their −3 dB point at about 20.95 kHz whatever the
precision, and spend the extra precision on stopband depth alone.

### Going down: 96 and 192 kHz to 48 kHz

A high-resolution file on a 48 kHz device. Every tone above 24 kHz in the source must vanish.

| Setting | 96 → 48: worst alias (dB) | 96 → 48: landing ≤ 20 kHz (dB) | 192 → 48: worst alias (dB) | 192 → 48: landing ≤ 20 kHz (dB) | −0.1 dB (kHz) | 192 → 48: CPU (%) |
|---|---:|---:|---:|---:|---:|---:|
| **Resonate High** | −136.6 | −152.3 | −136.6 | −153.1 | 22.67 | 0.248 |
| SoXR HQ | −127.9 | −139.0 | −133.7 | −135.3 | 22.34 | 0.072 |
| **Resonate Very high** | −185.8 | −205.9 | −185.8 | −205.9 | 23.23 | 0.332 |
| SoXR VHQ | −177.3 | −208.3 | −183.9 | −183.9 | 22.39 | 0.084 |
| SoXR 32-bit | −216.7 | −238.0 | −215.7 | −215.7 | 22.40 | 0.097 |

![Magnitude response and rejection, 96 kHz to 48 kHz](resampling/response-96-to-48.svg)

![Magnitude response and rejection, 192 kHz to 48 kHz](resampling/response-192-to-48.svg)

SoXR divides a large ratio into stages, so at 192 to 48 kHz its VHQ rejection rises again past
about 72 kHz, where its last stage's images sit; Resonate's stage one filters at the input's own
rate there and keeps the same stopband from the band edge to the input's Nyquist. At an integer
ratio, SoXR QQ (cubic interpolation) passes everything above Nyquist at 0 dB: it applies no
anti-alias filter.

### Phase, ringing and latency

| Setting | 44.1 → 48: delay spread (µs) | Ringing before / after (ms) | Energy over 1 ms early (dB) | Latency 44.1 → 48 (ms) | Latency 48 → 44.1 (ms) |
|---|---:|---:|---:|---:|---:|
| **Resonate High** | 0.0 | 2.65 / 2.65 | −44.2 | 6.26 | 5.98 |
| **Resonate High, minimum** | 58.7 | 0.08 / 4.77 | < −200 | 3.47 | 3.17 |
| **Resonate Very high** | 0.0 | 5.67 / 5.67 | −35.3 | 10.75 | 10.44 |
| **Resonate Very high, intermediate** | 38.7 | 4.06 / 8.23 | −61.1 | 12.56 | 12.25 |
| **Resonate Very high, minimum** | 77.5 | 0.10 / 10.88 | < −200 | 3.63 | 3.31 |
| SoXR HQ | 0.0 | 2.15 / 2.15 | −50.8 | 18.39 | 3.04 |
| SoXR VHQ | 0.0 | 2.52 / 2.52 | −46.5 | 8.41 | 30.85 |
| SoXR VHQ, intermediate | 38.0 | 1.98 / 3.62 | −75.6 | 34.35 | 33.31 |
| SoXR VHQ, minimum | 76.1 | 0.08 / 4.75 | < −200 | 5.40 | 27.85 |

- Both libraries' minimum phase removes pre-echo entirely and spreads 20 Hz–20 kHz over about
  77 µs; Resonate's minimum-phase Very high rings for twice as long afterwards (10.9 ms against
  4.8 ms), the price of its narrower transition.
- Resonate's latency is its kernel's lead plus one FFT partition and the interpolator's lookahead,
  the same at every ratio: 2.9 to 3.5 ms on top of the kernel. SoXR's moves with the ratio, since
  it fills a block before it filters: VHQ's from 2.8 ms to 31 ms, VHQ steep's to 143 ms. Minimum
  phase brings Resonate to about 3 ms at every ratio; SoXR's minimum phase reaches 0.12 ms at 96
  to 48 kHz but waits 5.4 to 27.9 ms at the fractional ratios.
- Resonate's intermediate phase waits longer than its linear phase (12.56 ms against 10.75 ms),
  since its window keeps 1.25 half-widths of lead; the lead is genuinely part of the response, and
  trimming it to the stopband's level costs 14 dB of rejection.

### Throughput and construction

Stereo white noise at −6 dBFS, 4 s of it in blocks of 1 024 frames, the best of five runs, on an
idle machine: the share of one core a second of audio costs.

#### Throughput, percent of one core per second of stereo audio

| Setting | 44.1 kHz to 48 kHz | 48 kHz to 44.1 kHz | 96 kHz to 48 kHz | 96 kHz to 44.1 kHz | 192 kHz to 48 kHz | 44.1 kHz to 96 kHz | 48 kHz to 96 kHz |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| **Resonate Fast** | 0.068 | 0.064 | 0.083 | 0.087 | 0.113 | 0.133 | 0.124 |
| **Resonate Balanced** | 0.097 | 0.095 | 0.128 | 0.140 | 0.199 | 0.193 | 0.175 |
| **Resonate High** | 0.145 | 0.147 | 0.124 | 0.171 | 0.248 | 0.205 | 0.118 |
| **Resonate Very high** | 0.165 | 0.169 | 0.141 | 0.186 | 0.332 | 0.225 | 0.132 |
| **Resonate High, minimum** | 0.143 | 0.145 | 0.126 | 0.168 | 0.247 | 0.206 | 0.115 |
| **Resonate Very high, intermediate** | 0.172 | 0.178 | 0.155 | 0.203 | 0.319 | 0.255 | 0.146 |
| **Resonate Very high, minimum** | 0.162 | 0.165 | 0.148 | 0.185 | 0.308 | 0.259 | 0.166 |
| SoXR QQ | 0.023 | 0.022 | 0.026 | 0.025 | 0.033 | 0.043 | 0.044 |
| SoXR LQ | 0.045 | 0.050 | 0.041 | 0.062 | 0.069 | 0.095 | 0.044 |
| SoXR MQ | 0.050 | 0.052 | 0.043 | 0.064 | 0.071 | 0.065 | 0.045 |
| SoXR 16-bit | 0.051 | 0.052 | 0.043 | 0.064 | 0.071 | 0.069 | 0.045 |
| SoXR HQ | 0.053 | 0.056 | 0.044 | 0.069 | 0.072 | 0.070 | 0.047 |
| SoXR 24-bit | 0.053 | 0.054 | 0.038 | 0.064 | 0.080 | 0.078 | 0.040 |
| SoXR VHQ | 0.054 | 0.059 | 0.039 | 0.068 | 0.084 | 0.079 | 0.041 |
| SoXR 32-bit | 0.061 | 0.064 | 0.045 | 0.073 | 0.097 | 0.090 | 0.045 |
| SoXR VHQ, steep | 0.073 | 0.072 | 0.052 | 0.082 | 0.097 | 0.101 | 0.058 |
| SoXR VHQ, intermediate | 0.059 | 0.062 | 0.048 | 0.072 | 0.092 | 0.084 | 0.048 |
| SoXR VHQ, minimum | 0.054 | 0.059 | 0.039 | 0.068 | 0.084 | 0.079 | 0.041 |

#### Construction, milliseconds

| Setting | First, 44.1 kHz to 48 kHz | Again, 44.1 kHz to 48 kHz | First, 192 kHz to 48 kHz | Again, 192 kHz to 48 kHz |
| --- | ---: | ---: | ---: | ---: |
| **Resonate Fast** | 0.15 | 0.00 | 0.00 | 0.00 |
| **Resonate Balanced** | 0.30 | 0.00 | 0.01 | 0.00 |
| **Resonate High** | 0.75 | 0.01 | 1.32 | 0.01 |
| **Resonate Very high** | 1.62 | 0.01 | 3.05 | 0.01 |
| **Resonate High, minimum** | 61.39 | 0.01 | 0.70 | 0.01 |
| **Resonate Very high, intermediate** | 133.07 | 0.01 | 2.78 | 0.01 |
| **Resonate Very high, minimum** | 130.78 | 0.01 | 1.46 | 0.01 |
| SoXR QQ | 0.01 | 0.00 | 0.01 | 0.00 |
| SoXR LQ | 0.05 | 0.05 | 0.01 | 0.00 |
| SoXR MQ | 0.03 | 0.01 | 0.01 | 0.01 |
| SoXR 16-bit | 0.01 | 0.01 | 0.01 | 0.01 |
| SoXR HQ | 0.02 | 0.02 | 0.01 | 0.01 |
| SoXR 24-bit | 0.04 | 0.04 | 0.03 | 0.02 |
| SoXR VHQ | 0.04 | 0.04 | 0.03 | 0.02 |
| SoXR 32-bit | 0.06 | 0.06 | 0.05 | 0.05 |
| SoXR VHQ, steep | 0.20 | 0.19 | 0.18 | 0.18 |
| SoXR VHQ, intermediate | 0.45 | 0.40 | 0.40 | 0.39 |
| SoXR VHQ, minimum | 0.40 | 0.38 | 0.37 | 0.37 |

- **Throughput barely moves with SoXR's precision**: 32-bit costs 0.061 % at 44.1 to 48 kHz, only
  a seventh more than HQ, because a longer filter is one larger FFT. Resonate's High and Very high
  now work the same way, so Very high costs only a seventh more than High; Fast and Balanced still
  sum each output frame over their whole kernel, which is cheap at 39 and 95 taps.
- **Going down by a large ratio is still where the gap is widest**, at 4 times VHQ for Very high
  at 192 to 48 kHz: stage one runs at the input's rate, 192 000 samples a second through the FFT,
  where SoXR halves the rate twice first.
- **Intermediate phase costs within a tenth of linear phase**, though its kernel spans 1 040
  taps where linear's spans 640, since an FFT's cost grows slowly with length; minimum phase costs
  what linear does.
- **The timings vary by about a tenth between runs on the same code** (Very high at 192 to 48 kHz
  measured 0.273 % in one session and 0.332 % in two later ones); the tables are the latest run.
  SoXR's vary by a few hundredths.
- **Construction**: Resonate designs a kernel's spectra and the interpolator's table once and keeps
  them, so a build after the first is free (0.01 ms); the first costs 0.75 ms for High and 1.6 ms
  for Very high at 44.1 to 48 kHz, 3.1 ms at 192 kHz, and a shaped phase is designed once a run
  (62 ms for High minimum, 132 ms for Very high's). SoXR designs afresh each time but in under half
  a millisecond.

## What changed in Resonate

### The minimum and intermediate phase kernels keep the linear kernel's passband

`phase.rs` folds the linear kernel to minimum phase through the real cepstrum. It took the
logarithm of the spectrum floored at 1e-30, so every null of the stopband became a spike of the
log spectrum, the cepstrum aliased, and the truncated response strayed in the passband. The log is
now floored a hundredth under the Kaiser stopband the kernel's β promises, over an FFT padded
32-fold rather than 16-fold. A higher floor flattens the passband further, but the flat floor sums
over every polyphase image: at a tenth under the stopband, High's total stopband energy rose
15 dB and its SNR fell by as much. Two tests hold both sides
(`a_shaped_filter_keeps_the_stopband_and_passband_the_linear_one_promises`,
`a_shaped_filter_leaks_hardly_more_into_the_stopband_than_the_linear_one`).

| 44.1 kHz to 48 kHz | Ripple to 10 kHz, before (dB) | After (dB) | Deviation to 20 kHz, before (dB) | After (dB) |
|---|---:|---:|---:|---:|
| Resonate High, minimum | 8.6e-5 | 5.5e-8 | 0.0007 | < 0.0001 |
| Resonate Very high, intermediate | 1.0e-7 | 3.4e-9 | < 0.0001 | < 0.0001 |
| Resonate Very high, minimum | 4.1e-7 | 1.3e-8 | < 0.0001 | < 0.0001 |

### High and Very high resample in two stages

The single polyphase stage summed every output frame over the whole kernel, stretched by the ratio
going down: 640 taps a channel for Very high at 44.1 to 48 kHz, 2 560 at 192 to 48 kHz. The loop
was bound by its loads at about 3.5 multiply-adds a cycle, so no reordering of it would close the
gap; the work had to shrink. High and Very high now run:

1. **Stage one** (`prefilter.rs`): the same kernel, convolved by uniformly partitioned overlap-save
   at twice the input rate (at the input rate where the output is at most half of it), in
   partitions of 256 samples, 512 from 176.4 kHz. Before it is sampled, the kernel is convolved
   with a guard that is flat to the band edge and stops at the oversampled Nyquist: sampled bare
   at exactly twice the input rate, the window's far sidelobes folded onto the tone and its first
   image, and High lost 6 dB under 20 kHz.
2. **Stage two**: the polyphase engine with a short linear-phase interpolator (29 to 37 taps)
   passing half the oversampled band and stopping from one and a half of it. Where every output
   lands on a stage-one sample (96 to 48, 192 to 48, 48 to 96 kHz) the sample is picked, not
   summed.

The guard and the interpolator both reject 60 dB under the quality's own Kaiser stopband, a
thousandth of its ripple. The output stays aligned to the input, a track keeps its length to the
frame, and the exact-oracle test now checks each frame against a direct guarded convolution
followed by the interpolator, within 1e-12.

| | High 44.1 → 48, before | After | Very high 44.1 → 48, before | After | Very high 192 → 48, before | After |
|---|---:|---:|---:|---:|---:|---:|
| CPU, % of a core | 0.174 | **0.145** | 0.356 | **0.165** | 1.867 | **0.332** |
| Worst alias or image (dB) | −136.6 | −136.6 | −185.8 | −185.8 | −185.8 | −185.8 |
| Worst landing ≤ 20 kHz (dB) | −156.6 | **−158.3** | −208.5 | **−210.7** | −205.6 | **−205.9** |
| SNR at 1 kHz (dB) | 162.7 | **196.6** | 218.5 | **246.4** | 257.6 | 253.2 |
| Worst SNR 20 Hz–20 kHz (dB) | 146.8 | **147.1** | 201.8 | **202.7** | 257.6 | 253.2 |
| Null 20 Hz–10 kHz (dB) | −160.0 | **−168.8** | −210.2 | **−211.2** | −211.8 | −211.0 |
| Ripple 20 Hz–10 kHz (dB) | 5.1e-8 | 5.7e-8 | 4.2e-10 | 4.2e-10 | 4.2e-10 | 4.6e-10 |
| Streaming latency (ms) | 2.90 | 6.26 | 7.26 | 10.75 | 6.67 | 9.55 |

Every other rate pair and phase moved the same way: CPU fell 1.2 to 9.4 times (Very high
intermediate at 192 to 48 kHz from 3.006 % to 0.319 %), 97 figures of the tables below improved,
and the passband edges, rejection, ringing and pre-echo held. Fifteen figures moved by a hair the
other way, each at a level nothing hears: ripple by up to 1e-10 dB at Very high and 2e-8 dB at
High, High's worst alias at 96 to 48 kHz from −137.2 to −136.6 dB, Very high's null at 96 to
48 kHz from −214.2 to −211.0 dB. They are the design now delivered exactly: the kernel's response
on stage one's grid matches the continuous Kaiser design's own ripple (4.95e-10 dB for Very high,
1.46e-7 dB for High, between 20 Hz and 10 kHz), where the single stage's 160 sampled phases had
added an aliasing error that happened to lower these few figures. SNR above 250 dB, the f64
rounding floor, fell by up to 15 dB at a few pairs (an FFT rounds differently from a dot
product), the lowest of them from 206.8 to 205.4 dB. The latency rose by a partition, a guard's
lead and the interpolator's lookahead, 2.9 to 3.5 ms; it stays exact in `latency_frames`.

### Fast and Balanced start their stopband at Nyquist

At 16 and 32 taps a side, Fast's and Balanced's transition bands were wider than the room their
cutoffs leave under Nyquist, so their stopbands began past it and −39 and −36 dB got through just
above the band edge (landing between 20 and 24 kHz). Two cures were measured against leaving them,
each found empirically by `soxr-comparison --fast-and-balanced` rather than from the Kaiser
formula: the highest cutoff, and the fewest taps, for which nothing past Nyquist at any rate pair
is louder than 1 dB above the kernel's own stopband (−63.1 dB for Fast, −90.4 dB for Balanced).
All six settings and SoXR's nearest recipes were then measured and timed in one run (44.1 to
48 kHz where a column names no pair):

| Setting | Worst alias or image, any pair (dB) | Worst landing ≤ 20 kHz, any pair (dB) | −0.1 dB (kHz) | −3 dB (kHz) | At 20 kHz (dB) | Worst SNR, any pair (dB) | Null 20 Hz–10 kHz (dB) | Latency (ms) | CPU 44.1 → 48 (%) | CPU 192 → 48 (%) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Fast as it was, 16 taps a side | -39.4 | -66.3 | 16.96 | 19.25 | -7.05 | 65.8 | -74.8 | 0.36 | 0.066 | 0.105 |
| Fast narrowed, 16 taps a side, cutoff 0.8750 | -62.2 | -67.6 | 16.86 | 18.63 | -11.75 | 67.2 | -74.4 | 0.36 | 0.065 | 0.105 |
| **Fast lengthened, 19 taps a side, cutoff 0.9000 (now Fast)** | -62.6 | -68.4 | 17.00 | 19.34 | -7.26 | 67.8 | -74.7 | 0.43 | 0.069 | 0.114 |
| Balanced as it was, 32 taps a side | -36.2 | -94.3 | 19.32 | 20.36 | -1.25 | 94.4 | -103.3 | 0.73 | 0.084 | 0.153 |
| Balanced narrowed, 32 taps a side, cutoff 0.9100 | -90.1 | -97.2 | 18.11 | 19.70 | -5.37 | 95.8 | -102.9 | 0.73 | 0.085 | 0.153 |
| **Balanced lengthened, 47 taps a side, cutoff 0.9400 (now Balanced)** | -89.7 | -98.7 | 19.77 | 20.48 | -0.43 | 97.4 | -106.4 | 1.07 | 0.098 | 0.204 |
| SoXR LQ | -103.9 | -110.4 | 15.41 | 17.47 | -22.04 | 109.3 | -115.1 | 0.48 | 0.046 | 0.069 |
| SoXR MQ | -103.5 | -111.4 | 18.08 | 20.95 | -0.19 | 108.8 | -96.2 | 12.45 | 0.049 | 0.070 |
| SoXR 16-bit | -103.5 | -110.2 | 20.51 | 20.97 | -0.00 | 108.9 | -109.2 | 12.45 | 0.051 | 0.070 |
| SoXR HQ | -126.6 | -134.3 | 20.53 | 20.96 | -0.01 | 130.7 | -131.3 | 18.39 | 0.053 | 0.072 |

Narrowing closes the leak for no CPU but takes the top octave: Fast's −3 dB point falls from
19.25 to 18.63 kHz and its level at 20 kHz from −7.05 to −11.75 dB, Balanced's from −1.25 to
−5.37 dB. Lengthening closes it as well and widens the passband, Balanced's above all (at 20 kHz
−0.43 dB against −1.25 dB), raises the worst SNR by 2 and 3 dB and deepens Balanced's null by
3 dB, for 5 % and 17 % more CPU at 44.1 to 48 kHz (9 % and 33 % at 192 to 48 kHz) and 0.07 and
0.34 ms more latency. Fast and Balanced are now 19 and 47 taps a side; Balanced still costs a third
less than High.

## Findings left open

- **High and Very high still cost 2.5 to 4 times SoXR HQ's and VHQ's CPU**, now in
  `docs/PERFORMANCE.md`: stage two's 29 to 37 taps a frame are about a third of it (as timed
  while building it), and larger partitions would cost more latency.
- **No Resonate level reaches SoXR 32-bit's −207 dB stopband or VHQ steep's 21.75 kHz passband.**
  Both are beyond what any converter resolves; a fifth level would only matter to someone
  measuring rather than listening.

## Every measurement

The tables the harness prints, every setting at every rate pair. Resonate's settings are in bold.

### Frequency domain, 44.1 kHz to 48 kHz

| Setting | Ripple 20 Hz–10 kHz (dB) | Deviation 20 Hz–20 kHz (dB) | At 20 kHz (dB) | −0.1 dB (kHz) | −3 dB (kHz) | Worst alias or image (dB) | Worst landing ≤ 20 kHz (dB) | SNR at 1 kHz (dB) | Worst SNR 20 Hz–20 kHz (dB) | Null 20 Hz–20 kHz (dB) | Null 20 Hz–10 kHz (dB) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| **Resonate Fast** | 3.2e-3 | 7.2636 | -7.3 | 17.00 | 19.34 | -62.8 | -76.9 | 80.3 | 67.8 | -21.8 | -74.7 |
| **Resonate Balanced** | 7.8e-5 | 0.4267 | -0.4 | 19.77 | 20.48 | -90.0 | -109.1 | 119.6 | 97.4 | -43.2 | -106.4 |
| **Resonate High** | 5.7e-8 | < 0.0001 | -0.0 | 20.83 | 21.16 | -136.6 | -158.3 | 196.6 | 147.1 | -155.1 | -168.8 |
| **Resonate Very high** | 4.2e-10 | < 0.0001 | -0.0 | 21.34 | 21.51 | -185.8 | -210.7 | 246.4 | 202.7 | -208.0 | -211.2 |
| **Resonate High, minimum** | 7.6e-8 | < 0.0001 | -0.0 | 20.83 | 21.16 | -136.5 | -157.2 | 196.6 | 146.9 | -7.4 | -21.9 |
| **Resonate Very high, intermediate** | 3.4e-9 | < 0.0001 | -0.0 | 21.34 | 21.51 | -185.8 | -207.4 | 246.5 | 198.8 | -8.6 | -25.4 |
| **Resonate Very high, minimum** | 1.3e-8 | < 0.0001 | -0.0 | 21.34 | 21.51 | -185.8 | -211.2 | 246.6 | 202.3 | -5.8 | -19.4 |
| SoXR QQ | 4.2e-1 | 5.2375 | -5.2 | 6.65 | 16.94 | -7.3 | -18.0 | 110.7 | 9.6 | -19.5 | -39.4 |
| SoXR LQ | 3.2e-5 | 22.0392 | -22.0 | 15.41 | 17.47 | -105.1 | -115.8 | 119.2 | 109.6 | -17.5 | -115.1 |
| SoXR MQ | 7.3e-4 | 0.1872 | -0.2 | 18.08 | 20.95 | -109.6 | -128.0 | 114.8 | 108.9 | -49.6 | -96.2 |
| SoXR 16-bit | 5.5e-5 | 0.0043 | -0.0 | 20.51 | 20.97 | -109.3 | -126.6 | 112.8 | 111.4 | -83.0 | -109.2 |
| SoXR HQ | 4.9e-6 | 0.0078 | -0.0 | 20.53 | 20.96 | -132.0 | -141.5 | 133.1 | 131.1 | -77.7 | -131.3 |
| SoXR 24-bit | 2.4e-7 | 0.0048 | -0.0 | 20.56 | 20.95 | -158.9 | -177.6 | 163.3 | 160.4 | -81.9 | -158.8 |
| SoXR VHQ | 1.4e-8 | 0.0062 | -0.0 | 20.57 | 20.95 | -183.6 | -201.4 | 187.0 | 184.1 | -79.7 | -182.4 |
| SoXR 32-bit | 7.2e-9 | 0.0078 | -0.0 | 20.58 | 20.94 | -207.0 | -222.0 | 212.3 | 207.1 | -77.7 | -190.7 |
| SoXR VHQ, steep | 1.3e-8 | 0.0017 | -0.0 | 21.75 | 21.82 | -181.6 | -214.6 | 189.9 | 183.9 | -91.1 | -183.0 |
| SoXR VHQ, intermediate | 1.4e-8 | 0.0062 | -0.0 | 20.57 | 20.95 | -183.7 | -201.5 | 187.0 | 183.9 | -8.7 | -26.0 |
| SoXR VHQ, minimum | 1.7e-8 | 0.0062 | -0.0 | 20.57 | 20.95 | -183.6 | -201.5 | 187.0 | 184.1 | -5.9 | -20.0 |

### Time domain, 44.1 kHz to 48 kHz

| Setting | Delay at 1 kHz (µs) | Delay spread 20 Hz–20 kHz (µs) | Impulse peak offset (µs) | Ringing before peak (ms) | Ringing after peak (ms) | Energy over 1 ms before peak (dB) | Streaming latency (ms) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| **Resonate Fast** | -0.0 | 0.0 | 0.0 | 0.42 | 0.42 | < -200 | 0.43 |
| **Resonate Balanced** | 0.0 | 0.0 | 0.0 | 1.06 | 1.06 | -93.8 | 1.07 |
| **Resonate High** | -0.0 | 0.0 | 0.0 | 2.65 | 2.65 | -44.2 | 6.26 |
| **Resonate Very high** | 0.0 | 0.0 | 0.0 | 5.67 | 5.67 | -35.3 | 10.75 |
| **Resonate High, minimum** | -15.0 | 58.7 | 0.0 | 0.08 | 4.77 | < -200 | 3.47 |
| **Resonate Very high, intermediate** | -12.6 | 38.7 | 0.0 | 4.06 | 8.23 | -61.1 | 12.56 |
| **Resonate Very high, minimum** | -16.8 | 77.5 | 0.0 | 0.10 | 10.88 | < -200 | 3.63 |
| SoXR QQ | 0.0 | 0.0 | 0.0 | 0.04 | 0.04 | < -200 | 0.02 |
| SoXR LQ | -0.0 | 0.0 | 0.0 | 0.46 | 0.46 | < -200 | 0.48 |
| SoXR MQ | -0.0 | 0.0 | 0.0 | 1.85 | 1.85 | -54.4 | 12.45 |
| SoXR 16-bit | -0.0 | 0.0 | 0.0 | 1.85 | 1.85 | -54.2 | 12.45 |
| SoXR HQ | 0.0 | 0.0 | 0.0 | 2.15 | 2.15 | -50.8 | 18.39 |
| SoXR 24-bit | 0.0 | 0.0 | 0.0 | 2.33 | 2.33 | -48.3 | 4.92 |
| SoXR VHQ | 0.0 | 0.0 | 0.0 | 2.52 | 2.52 | -46.5 | 8.41 |
| SoXR 32-bit | 0.0 | 0.0 | 0.0 | 2.69 | 2.69 | -45.1 | 18.44 |
| SoXR VHQ, steep | 0.0 | 0.0 | 0.0 | 11.77 | 11.77 | -31.6 | 24.83 |
| SoXR VHQ, intermediate | -13.4 | 38.0 | 0.0 | 1.98 | 3.62 | -75.6 | 34.35 |
| SoXR VHQ, minimum | -26.8 | 76.1 | -20.8 | 0.08 | 4.75 | < -200 | 5.40 |

### Frequency domain, 48 kHz to 44.1 kHz

| Setting | Ripple 20 Hz–10 kHz (dB) | Deviation 20 Hz–20 kHz (dB) | At 20 kHz (dB) | −0.1 dB (kHz) | −3 dB (kHz) | Worst alias or image (dB) | Worst landing ≤ 20 kHz (dB) | SNR at 1 kHz (dB) | Worst SNR 20 Hz–20 kHz (dB) | Null 20 Hz–20 kHz (dB) | Null 20 Hz–10 kHz (dB) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| **Resonate Fast** | 3.2e-3 | 7.2636 | -7.3 | 17.00 | 19.34 | -62.9 | -71.0 | 77.8 | 72.6 | -21.8 | -74.8 |
| **Resonate Balanced** | 7.8e-5 | 0.4267 | -0.4 | 19.77 | 20.48 | -89.8 | -101.4 | 127.9 | 102.4 | -43.2 | -106.1 |
| **Resonate High** | 4.8e-8 | < 0.0001 | -0.0 | 20.83 | 21.16 | -136.4 | -150.9 | 197.1 | 153.5 | -155.8 | -171.4 |
| **Resonate Very high** | 4.3e-10 | < 0.0001 | -0.0 | 21.34 | 21.51 | -185.7 | -207.7 | 247.7 | 210.2 | -208.3 | -211.2 |
| **Resonate High, minimum** | 6.4e-8 | < 0.0001 | -0.0 | 20.83 | 21.16 | -136.5 | -151.1 | 197.1 | 153.1 | -7.4 | -21.9 |
| **Resonate Very high, intermediate** | 3.4e-9 | < 0.0001 | -0.0 | 21.34 | 21.51 | -185.8 | -203.1 | 247.7 | 206.6 | -8.6 | -25.4 |
| **Resonate Very high, minimum** | 1.3e-8 | < 0.0001 | -0.0 | 21.34 | 21.51 | -185.7 | -207.8 | 247.7 | 210.4 | -5.8 | -19.4 |
| SoXR QQ | 3.1e-1 | 3.9418 | -3.9 | 7.25 | 18.42 | -3.9 | -9.4 | 113.6 | 11.9 | -21.7 | -42.1 |
| SoXR LQ | 9.5e-3 | 23.2641 | -23.3 | 15.15 | 17.43 | -109.5 | -119.4 | 113.6 | 110.1 | -17.4 | -73.6 |
| SoXR MQ | 4.3e-3 | 0.2153 | -0.2 | 17.24 | 20.95 | -109.8 | -122.0 | 113.9 | 109.3 | -48.0 | -80.8 |
| SoXR 16-bit | 5.6e-5 | 0.0051 | -0.0 | 20.51 | 20.97 | -109.4 | -121.2 | 115.7 | 108.9 | -81.4 | -110.1 |
| SoXR HQ | 3.3e-6 | 0.0078 | -0.0 | 20.53 | 20.96 | -130.6 | -139.8 | 133.5 | 130.7 | -77.6 | -131.3 |
| SoXR 24-bit | 2.0e-7 | 0.0016 | -0.0 | 20.56 | 20.95 | -158.8 | -170.4 | 164.1 | 160.3 | -91.5 | -159.0 |
| SoXR VHQ | 1.5e-8 | 0.0023 | -0.0 | 20.57 | 20.95 | -182.0 | -195.1 | 187.5 | 185.7 | -88.3 | -182.3 |
| SoXR 32-bit | 8.4e-10 | 0.0031 | -0.0 | 20.59 | 20.94 | -208.5 | -220.5 | 213.1 | 209.7 | -85.7 | -205.4 |
| SoXR VHQ, steep | 1.2e-8 | 0.0013 | -0.0 | 21.75 | 21.82 | -181.4 | -206.5 | 187.0 | 184.2 | -93.3 | -183.6 |
| SoXR VHQ, intermediate | 1.8e-8 | 0.0023 | -0.0 | 20.57 | 20.95 | -182.1 | -195.2 | 187.5 | 185.7 | -8.7 | -25.7 |
| SoXR VHQ, minimum | 4.8e-8 | 0.0023 | -0.0 | 20.57 | 20.95 | -182.0 | -195.2 | 187.5 | 185.6 | -5.9 | -19.8 |

### Time domain, 48 kHz to 44.1 kHz

| Setting | Delay at 1 kHz (µs) | Delay spread 20 Hz–20 kHz (µs) | Impulse peak offset (µs) | Ringing before peak (ms) | Ringing after peak (ms) | Energy over 1 ms before peak (dB) | Streaming latency (ms) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| **Resonate Fast** | -0.0 | 0.0 | 0.0 | 0.41 | 0.41 | < -200 | 0.44 |
| **Resonate Balanced** | -0.0 | 0.0 | 0.0 | 1.04 | 1.04 | -93.4 | 1.08 |
| **Resonate High** | 0.0 | 0.0 | 0.0 | 2.52 | 2.52 | -43.9 | 5.98 |
| **Resonate Very high** | 0.0 | 0.0 | 0.0 | 5.69 | 5.69 | -36.0 | 10.44 |
| **Resonate High, minimum** | -15.0 | 58.7 | 0.0 | 0.09 | 4.51 | < -200 | 3.17 |
| **Resonate Very high, intermediate** | -12.6 | 38.7 | 0.0 | 4.04 | 8.28 | -61.0 | 12.25 |
| **Resonate Very high, minimum** | -16.8 | 77.5 | 0.0 | 0.11 | 10.50 | < -200 | 3.31 |
| SoXR QQ | -0.0 | 0.0 | 0.0 | 0.02 | 0.02 | < -200 | 0.04 |
| SoXR LQ | 0.0 | 0.0 | 0.0 | 0.50 | 0.50 | -139.0 | 9.81 |
| SoXR MQ | -0.0 | 0.0 | 0.0 | 1.86 | 1.86 | -58.0 | 17.62 |
| SoXR 16-bit | -0.0 | 0.0 | 0.0 | 1.86 | 1.86 | -57.8 | 17.62 |
| SoXR HQ | 0.0 | 0.0 | 0.0 | 2.04 | 2.04 | -52.9 | 3.04 |
| SoXR 24-bit | -0.0 | 0.0 | 0.0 | 2.36 | 2.36 | -49.8 | 5.10 |
| SoXR VHQ | 0.0 | 0.0 | 0.0 | 2.47 | 2.47 | -47.4 | 30.85 |
| SoXR 32-bit | 0.0 | 0.0 | 0.0 | 2.74 | 2.74 | -45.6 | 23.77 |
| SoXR VHQ, steep | 0.0 | 0.0 | 0.0 | 11.75 | 11.75 | -30.1 | 134.27 |
| SoXR VHQ, intermediate | -15.8 | 38.6 | 0.0 | 1.90 | 3.67 | -74.5 | 33.31 |
| SoXR VHQ, minimum | -31.5 | 77.2 | -22.7 | 0.09 | 4.60 | < -200 | 27.85 |

### Frequency domain, 96 kHz to 48 kHz

| Setting | Ripple 20 Hz–10 kHz (dB) | Deviation 20 Hz–20 kHz (dB) | At 20 kHz (dB) | −0.1 dB (kHz) | −3 dB (kHz) | Worst alias or image (dB) | Worst landing ≤ 20 kHz (dB) | SNR at 1 kHz (dB) | Worst SNR 20 Hz–20 kHz (dB) | Null 20 Hz–20 kHz (dB) | Null 20 Hz–10 kHz (dB) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| **Resonate Fast** | 3.5e-3 | 0.4024 | -0.4 | 18.69 | 21.05 | -63.7 | -71.6 | 271.7 | 258.4 | -43.7 | -74.9 |
| **Resonate Balanced** | 9.1e-5 | 0.0002 | -0.0 | 21.52 | 22.29 | -89.7 | -101.7 | 275.5 | 258.8 | -104.9 | -104.8 |
| **Resonate High** | 1.1e-7 | < 0.0001 | -0.0 | 22.67 | 23.03 | -136.6 | -152.3 | 250.5 | 250.5 | -162.2 | -168.1 |
| **Resonate Very high** | 4.6e-10 | < 0.0001 | -0.0 | 23.23 | 23.41 | -185.8 | -205.9 | 280.4 | 269.4 | -210.2 | -211.0 |
| **Resonate High, minimum** | 1.2e-7 | < 0.0001 | -0.0 | 22.67 | 23.03 | -136.6 | -152.4 | 262.9 | 253.7 | -7.8 | -24.3 |
| **Resonate Very high, intermediate** | 2.4e-9 | < 0.0001 | -0.0 | 23.23 | 23.41 | -185.8 | -203.1 | 262.3 | 255.8 | -8.5 | -27.8 |
| **Resonate Very high, minimum** | 9.5e-9 | < 0.0001 | -0.0 | 23.23 | 23.41 | -185.8 | -205.8 | 273.3 | 252.5 | -7.9 | -21.8 |
| SoXR QQ | 8.6e-8 | < 0.0001 | -0.0 | — | — | 0.0 | 0.0 | 154.9 | 153.3 | -151.9 | -152.0 |
| SoXR LQ | 1.7e-5 | 5.3675 | -5.4 | 16.78 | 19.17 | -109.3 | -128.4 | 135.4 | 135.1 | -23.6 | -126.6 |
| SoXR MQ | 1.3e-5 | < 0.0001 | 0.0 | 22.32 | 22.82 | -105.9 | -136.4 | 135.6 | 134.9 | -123.6 | -124.5 |
| SoXR 16-bit | 1.3e-5 | < 0.0001 | 0.0 | 22.32 | 22.82 | -105.9 | -136.4 | 135.6 | 134.9 | -123.6 | -124.5 |
| SoXR HQ | 2.2e-6 | < 0.0001 | -0.0 | 22.34 | 22.81 | -127.9 | -139.0 | 135.8 | 134.6 | -133.1 | -133.1 |
| SoXR 24-bit | 4.8e-8 | < 0.0001 | 0.0 | 22.37 | 22.80 | -152.3 | -184.8 | 201.1 | 191.4 | -172.3 | -173.9 |
| SoXR VHQ | 2.8e-9 | < 0.0001 | 0.0 | 22.39 | 22.80 | -177.3 | -208.3 | 224.8 | 215.1 | -197.2 | -200.3 |
| SoXR 32-bit | 8.8e-11 | < 0.0001 | 0.0 | 22.40 | 22.80 | -216.7 | -238.0 | 252.5 | 243.4 | -222.1 | -228.3 |
| SoXR VHQ, steep | 3.6e-10 | < 0.0001 | -0.0 | 23.67 | 23.75 | -192.3 | -229.5 | 241.0 | 230.4 | -215.0 | -219.0 |
| SoXR VHQ, intermediate | 2.7e-8 | < 0.0001 | -0.0 | 22.39 | 22.80 | -201.1 | -223.6 | 235.0 | 223.2 | -8.8 | -28.6 |
| SoXR VHQ, minimum | 1.0e-7 | < 0.0001 | -0.0 | 22.39 | 22.80 | -177.1 | -205.7 | 225.8 | 215.6 | -8.0 | -22.6 |

### Time domain, 96 kHz to 48 kHz

| Setting | Delay at 1 kHz (µs) | Delay spread 20 Hz–20 kHz (µs) | Impulse peak offset (µs) | Ringing before peak (ms) | Ringing after peak (ms) | Energy over 1 ms before peak (dB) | Streaming latency (ms) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| **Resonate Fast** | -0.0 | 0.0 | 0.0 | 0.38 | 0.38 | < -200 | 0.40 |
| **Resonate Balanced** | -0.0 | 0.0 | 0.0 | 0.96 | 0.96 | < -200 | 0.98 |
| **Resonate High** | 0.0 | 0.0 | 0.0 | 2.31 | 2.31 | -48.5 | 5.76 |
| **Resonate Very high** | 0.0 | 0.0 | 0.0 | 5.23 | 5.23 | -36.0 | 9.88 |
| **Resonate High, minimum** | -13.8 | 35.0 | 0.0 | 0.08 | 4.15 | < -200 | 3.20 |
| **Resonate Very high, intermediate** | -11.6 | 23.3 | 0.0 | 3.71 | 7.60 | -61.3 | 11.54 |
| **Resonate Very high, minimum** | -15.4 | 46.5 | 0.0 | 0.10 | 9.65 | < -200 | 3.33 |
| SoXR QQ | -0.0 | 0.0 | 0.0 | 0.00 | 0.00 | < -200 | 0.02 |
| SoXR LQ | -0.0 | 0.0 | 0.0 | 0.44 | 0.44 | < -200 | 3.97 |
| SoXR MQ | -0.0 | 0.0 | 0.0 | 1.65 | 1.65 | -60.2 | 8.34 |
| SoXR 16-bit | -0.0 | 0.0 | 0.0 | 1.65 | 1.65 | -60.2 | 8.34 |
| SoXR HQ | 0.0 | 0.0 | 0.0 | 1.85 | 1.85 | -54.2 | 13.26 |
| SoXR 24-bit | -0.0 | 0.0 | 0.0 | 2.12 | 2.12 | -50.6 | 16.68 |
| SoXR VHQ | -0.0 | 0.0 | 0.0 | 2.25 | 2.25 | -48.2 | 2.80 |
| SoXR 32-bit | -0.0 | 0.0 | 0.0 | 2.46 | 2.46 | -46.4 | 30.51 |
| SoXR VHQ, steep | -0.0 | 0.0 | 0.0 | 10.60 | 10.60 | -30.9 | 143.14 |
| SoXR VHQ, intermediate | -8.6 | 21.6 | 0.0 | 1.77 | 3.29 | -78.7 | 9.70 |
| SoXR VHQ, minimum | -17.2 | 43.3 | 0.0 | 0.08 | 4.04 | -197.0 | 0.12 |

### Frequency domain, 96 kHz to 44.1 kHz

| Setting | Ripple 20 Hz–10 kHz (dB) | Deviation 20 Hz–20 kHz (dB) | At 20 kHz (dB) | −0.1 dB (kHz) | −3 dB (kHz) | Worst alias or image (dB) | Worst landing ≤ 20 kHz (dB) | SNR at 1 kHz (dB) | Worst SNR 20 Hz–20 kHz (dB) | Null 20 Hz–20 kHz (dB) | Null 20 Hz–10 kHz (dB) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| **Resonate Fast** | 3.2e-3 | 7.2636 | -7.3 | 17.00 | 19.34 | -63.8 | -68.4 | 91.4 | 91.0 | -21.8 | -75.3 |
| **Resonate Balanced** | 7.8e-5 | 0.4267 | -0.4 | 19.77 | 20.48 | -89.9 | -98.7 | 132.0 | 116.3 | -43.2 | -107.2 |
| **Resonate High** | 4.8e-8 | < 0.0001 | -0.0 | 20.83 | 21.16 | -136.5 | -148.7 | 197.1 | 193.3 | -156.0 | -171.5 |
| **Resonate Very high** | 4.3e-10 | < 0.0001 | -0.0 | 21.34 | 21.51 | -185.8 | -201.1 | 247.7 | 239.4 | -208.4 | -211.2 |
| **Resonate High, minimum** | 6.4e-8 | < 0.0001 | -0.0 | 20.83 | 21.16 | -136.5 | -148.5 | 197.1 | 193.3 | -7.4 | -21.9 |
| **Resonate Very high, intermediate** | 3.4e-9 | < 0.0001 | -0.0 | 21.34 | 21.51 | -185.9 | -198.7 | 247.7 | 239.4 | -8.6 | -25.4 |
| **Resonate Very high, minimum** | 1.3e-8 | < 0.0001 | -0.0 | 21.34 | 21.51 | -185.8 | -201.2 | 247.7 | 239.4 | -5.8 | -19.4 |
| SoXR QQ | 2.1e-2 | 0.3349 | -0.3 | 14.49 | — | -0.4 | -0.7 | 136.9 | 33.9 | -42.5 | -65.4 |
| SoXR LQ | 9.5e-3 | 23.2641 | -23.3 | 15.15 | 17.43 | -110.4 | -110.4 | 113.6 | 110.1 | -17.4 | -73.6 |
| SoXR MQ | 4.3e-3 | 0.2153 | -0.2 | 17.24 | 20.95 | -110.1 | -111.4 | 113.9 | 109.3 | -48.0 | -80.8 |
| SoXR 16-bit | 5.6e-5 | 0.0051 | -0.0 | 20.51 | 20.97 | -109.7 | -110.2 | 115.7 | 108.9 | -81.4 | -110.1 |
| SoXR HQ | 3.4e-6 | 0.0078 | -0.0 | 20.53 | 20.96 | -131.0 | -134.3 | 134.1 | 131.3 | -77.6 | -131.5 |
| SoXR 24-bit | 2.0e-7 | 0.0016 | -0.0 | 20.56 | 20.95 | -159.0 | -162.5 | 164.1 | 160.3 | -91.5 | -159.0 |
| SoXR VHQ | 1.5e-8 | 0.0023 | -0.0 | 20.57 | 20.95 | -184.8 | -186.8 | 187.5 | 185.7 | -88.3 | -182.2 |
| SoXR 32-bit | 8.4e-10 | 0.0031 | -0.0 | 20.59 | 20.94 | -207.4 | -210.9 | 213.1 | 209.7 | -85.7 | -205.4 |
| SoXR VHQ, steep | 1.2e-8 | 0.0013 | -0.0 | 21.75 | 21.82 | -181.4 | -184.3 | 187.0 | 184.2 | -93.3 | -183.6 |
| SoXR VHQ, intermediate | 1.8e-8 | 0.0023 | -0.0 | 20.57 | 20.95 | -184.0 | -186.8 | 187.5 | 185.7 | -8.7 | -25.7 |
| SoXR VHQ, minimum | 4.8e-8 | 0.0023 | -0.0 | 20.57 | 20.95 | -184.8 | -186.8 | 187.5 | 185.7 | -5.9 | -19.8 |

### Time domain, 96 kHz to 44.1 kHz

| Setting | Delay at 1 kHz (µs) | Delay spread 20 Hz–20 kHz (µs) | Impulse peak offset (µs) | Ringing before peak (ms) | Ringing after peak (ms) | Energy over 1 ms before peak (dB) | Streaming latency (ms) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| **Resonate Fast** | -0.0 | 0.0 | 0.0 | 0.41 | 0.41 | < -200 | 0.44 |
| **Resonate Balanced** | -0.0 | 0.0 | 0.0 | 1.04 | 1.04 | -93.4 | 1.07 |
| **Resonate High** | 0.0 | 0.0 | 0.0 | 2.52 | 2.52 | -43.9 | 5.98 |
| **Resonate Very high** | 0.0 | 0.0 | 0.0 | 5.69 | 5.69 | -36.0 | 10.45 |
| **Resonate High, minimum** | -15.0 | 58.7 | 0.0 | 0.09 | 4.51 | < -200 | 3.18 |
| **Resonate Very high, intermediate** | -12.6 | 38.7 | 0.0 | 4.04 | 8.28 | -61.0 | 12.26 |
| **Resonate Very high, minimum** | -16.8 | 77.5 | 0.0 | 0.11 | 10.50 | < -200 | 3.32 |
| SoXR QQ | -0.0 | 0.0 | 0.0 | 0.00 | 0.00 | < -200 | 0.02 |
| SoXR LQ | 0.0 | 0.0 | 0.0 | 0.50 | 0.50 | -140.0 | 9.82 |
| SoXR MQ | -0.0 | 0.0 | 0.0 | 1.86 | 1.86 | -58.0 | 17.64 |
| SoXR 16-bit | -0.0 | 0.0 | 0.0 | 1.86 | 1.86 | -57.8 | 17.64 |
| SoXR HQ | -0.0 | 0.0 | 0.0 | 2.04 | 2.04 | -52.9 | 3.05 |
| SoXR 24-bit | -0.0 | 0.0 | 0.0 | 2.36 | 2.36 | -49.8 | 5.11 |
| SoXR VHQ | 0.0 | 0.0 | 0.0 | 2.47 | 2.47 | -47.4 | 30.86 |
| SoXR 32-bit | 0.0 | 0.0 | 0.0 | 2.74 | 2.74 | -45.6 | 23.78 |
| SoXR VHQ, steep | 0.0 | 0.0 | 0.0 | 11.75 | 11.75 | -30.1 | 134.28 |
| SoXR VHQ, intermediate | -15.8 | 38.6 | 0.0 | 1.90 | 3.67 | -74.5 | 33.32 |
| SoXR VHQ, minimum | -21.1 | 77.2 | 0.0 | 0.11 | 4.78 | < -200 | 27.85 |

### Frequency domain, 192 kHz to 48 kHz

| Setting | Ripple 20 Hz–10 kHz (dB) | Deviation 20 Hz–20 kHz (dB) | At 20 kHz (dB) | −0.1 dB (kHz) | −3 dB (kHz) | Worst alias or image (dB) | Worst landing ≤ 20 kHz (dB) | SNR at 1 kHz (dB) | Worst SNR 20 Hz–20 kHz (dB) | Null 20 Hz–20 kHz (dB) | Null 20 Hz–10 kHz (dB) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| **Resonate Fast** | 3.5e-3 | 0.4024 | -0.4 | 18.69 | 21.05 | -63.8 | -71.1 | 263.4 | 263.4 | -43.7 | -74.9 |
| **Resonate Balanced** | 8.5e-5 | 0.0002 | -0.0 | 21.52 | 22.29 | -89.8 | -101.2 | 260.7 | 259.2 | -105.5 | -105.6 |
| **Resonate High** | 1.1e-7 | < 0.0001 | -0.0 | 22.67 | 23.03 | -136.6 | -153.1 | 271.4 | 248.1 | -162.4 | -168.8 |
| **Resonate Very high** | 4.6e-10 | < 0.0001 | -0.0 | 23.23 | 23.41 | -185.8 | -205.9 | 253.2 | 253.2 | -210.2 | -211.0 |
| **Resonate High, minimum** | 1.1e-7 | < 0.0001 | -0.0 | 22.67 | 23.03 | -136.5 | -153.1 | 270.5 | 257.2 | -7.8 | -24.3 |
| **Resonate Very high, intermediate** | 2.5e-9 | < 0.0001 | -0.0 | 23.23 | 23.41 | -185.9 | -203.8 | 251.7 | 251.7 | -8.5 | -27.8 |
| **Resonate Very high, minimum** | 9.5e-9 | < 0.0001 | -0.0 | 23.23 | 23.41 | -185.8 | -205.9 | 274.9 | 262.5 | -7.9 | -21.8 |
| SoXR QQ | 8.6e-8 | < 0.0001 | -0.0 | — | — | 0.0 | 0.0 | 154.9 | 153.3 | -151.9 | -152.0 |
| SoXR LQ | 2.0e-5 | 5.3577 | -5.4 | 16.80 | 19.17 | -115.7 | -133.7 | 136.6 | 134.9 | -23.6 | -123.4 |
| SoXR MQ | 5.0e-6 | < 0.0001 | -0.0 | 22.33 | 22.82 | -111.0 | -135.3 | 135.8 | 133.7 | -128.4 | -129.7 |
| SoXR 16-bit | 5.0e-6 | < 0.0001 | -0.0 | 22.33 | 22.82 | -111.0 | -135.3 | 135.8 | 133.7 | -128.4 | -129.7 |
| SoXR HQ | 4.2e-6 | < 0.0001 | -0.0 | 22.36 | 22.81 | -133.7 | -135.3 | 135.2 | 134.0 | -126.2 | -128.8 |
| SoXR 24-bit | 8.8e-8 | < 0.0001 | -0.0 | 22.38 | 22.80 | -158.8 | -168.5 | 207.3 | 197.3 | -169.7 | -169.5 |
| SoXR VHQ | 1.3e-8 | < 0.0001 | -0.0 | 22.40 | 22.80 | -183.9 | -183.9 | 230.7 | 220.9 | -185.4 | -185.3 |
| SoXR 32-bit | 3.6e-10 | < 0.0001 | -0.0 | 22.41 | 22.80 | -215.7 | -215.7 | 255.9 | 247.5 | -216.1 | -216.2 |
| SoXR VHQ, steep | 1.2e-8 | < 0.0001 | -0.0 | 23.67 | 23.75 | -183.9 | -183.9 | 247.4 | 236.2 | -185.2 | -185.3 |
| SoXR VHQ, intermediate | 1.3e-8 | < 0.0001 | -0.0 | 22.40 | 22.80 | -183.9 | -183.9 | 239.9 | 228.8 | -8.7 | -28.3 |
| SoXR VHQ, minimum | 1.6e-8 | < 0.0001 | -0.0 | 22.40 | 22.80 | -183.9 | -183.9 | 232.1 | 221.6 | -8.0 | -22.3 |

### Time domain, 192 kHz to 48 kHz

| Setting | Delay at 1 kHz (µs) | Delay spread 20 Hz–20 kHz (µs) | Impulse peak offset (µs) | Ringing before peak (ms) | Ringing after peak (ms) | Energy over 1 ms before peak (dB) | Streaming latency (ms) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| **Resonate Fast** | -0.0 | 0.0 | 0.0 | 0.38 | 0.38 | < -200 | 0.40 |
| **Resonate Balanced** | 0.0 | 0.0 | 0.0 | 0.96 | 0.96 | < -200 | 0.98 |
| **Resonate High** | 0.0 | 0.0 | 0.0 | 2.31 | 2.31 | -48.5 | 5.50 |
| **Resonate Very high** | 0.0 | 0.0 | 0.0 | 5.23 | 5.23 | -36.0 | 9.55 |
| **Resonate High, minimum** | -13.8 | 35.0 | 0.0 | 0.08 | 4.15 | < -200 | 2.93 |
| **Resonate Very high, intermediate** | -11.6 | 23.3 | 0.0 | 3.71 | 7.60 | -61.3 | 11.21 |
| **Resonate Very high, minimum** | -15.4 | 46.5 | 0.0 | 0.10 | 9.65 | < -200 | 3.01 |
| SoXR QQ | -0.0 | 0.0 | 0.0 | 0.00 | 0.00 | < -200 | 0.02 |
| SoXR LQ | 0.0 | 0.0 | 0.0 | 0.46 | 0.46 | -136.4 | 2.78 |
| SoXR MQ | -0.0 | 0.0 | 0.0 | 1.73 | 1.73 | -58.4 | 4.99 |
| SoXR 16-bit | -0.0 | 0.0 | 0.0 | 1.73 | 1.73 | -58.4 | 4.99 |
| SoXR HQ | -0.0 | 0.0 | 0.0 | 1.88 | 1.88 | -53.1 | 9.70 |
| SoXR 24-bit | -0.0 | 0.0 | 0.0 | 2.17 | 2.17 | -49.9 | 12.93 |
| SoXR VHQ | 0.0 | 0.0 | 0.0 | 2.27 | 2.27 | -47.7 | 14.65 |
| SoXR 32-bit | 0.0 | 0.0 | 0.0 | 2.52 | 2.52 | -46.0 | 28.88 |
| SoXR VHQ, steep | -0.0 | 0.0 | 0.0 | 10.79 | 10.79 | -30.9 | 140.65 |
| SoXR VHQ, intermediate | -12.3 | 22.4 | 0.0 | 1.77 | 3.38 | -78.6 | 7.31 |
| SoXR VHQ, minimum | -14.2 | 44.8 | 0.0 | 0.08 | 4.35 | < -200 | 11.86 |

### Frequency domain, 44.1 kHz to 96 kHz

| Setting | Ripple 20 Hz–10 kHz (dB) | Deviation 20 Hz–20 kHz (dB) | At 20 kHz (dB) | −0.1 dB (kHz) | −3 dB (kHz) | Worst alias or image (dB) | Worst landing ≤ 20 kHz (dB) | SNR at 1 kHz (dB) | Worst SNR 20 Hz–20 kHz (dB) | Null 20 Hz–20 kHz (dB) | Null 20 Hz–10 kHz (dB) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| **Resonate Fast** | 3.2e-3 | 7.2636 | -7.3 | 17.00 | 19.34 | -62.8 | — | 80.2 | 67.8 | -21.8 | -74.7 |
| **Resonate Balanced** | 7.8e-5 | 0.4267 | -0.4 | 19.77 | 20.48 | -90.0 | — | 119.6 | 97.4 | -43.2 | -106.3 |
| **Resonate High** | 5.7e-8 | < 0.0001 | -0.0 | 20.83 | 21.16 | -136.6 | — | 196.6 | 147.1 | -155.1 | -168.8 |
| **Resonate Very high** | 4.2e-10 | < 0.0001 | -0.0 | 21.34 | 21.51 | -185.8 | — | 246.6 | 202.7 | -208.0 | -211.2 |
| **Resonate High, minimum** | 7.6e-8 | < 0.0001 | -0.0 | 20.83 | 21.16 | -136.5 | — | 196.6 | 146.9 | -7.4 | -21.9 |
| **Resonate Very high, intermediate** | 3.4e-9 | < 0.0001 | -0.0 | 21.34 | 21.51 | -185.8 | — | 246.6 | 198.8 | -8.6 | -25.4 |
| **Resonate Very high, minimum** | 1.3e-8 | < 0.0001 | -0.0 | 21.34 | 21.51 | -185.8 | — | 246.4 | 202.3 | -5.8 | -19.4 |
| SoXR QQ | 4.2e-1 | 5.2376 | -5.2 | 6.65 | 16.94 | -7.3 | — | 110.7 | 9.6 | -19.5 | -39.4 |
| SoXR LQ | 3.2e-5 | 22.0405 | -22.0 | 15.41 | 17.47 | -105.1 | — | 119.4 | 109.6 | -17.5 | -115.2 |
| SoXR MQ | 7.2e-4 | 0.1867 | -0.2 | 18.09 | 20.95 | -109.6 | — | 115.3 | 108.8 | -49.6 | -96.3 |
| SoXR 16-bit | 5.5e-5 | 0.0043 | -0.0 | 20.51 | 20.97 | -109.3 | — | 112.8 | 111.4 | -83.1 | -109.2 |
| SoXR HQ | 4.8e-6 | 0.0078 | -0.0 | 20.53 | 20.96 | -132.0 | — | 133.1 | 131.1 | -77.7 | -131.3 |
| SoXR 24-bit | 2.4e-7 | 0.0048 | -0.0 | 20.56 | 20.95 | -158.9 | — | 163.3 | 160.4 | -81.9 | -158.8 |
| SoXR VHQ | 1.4e-8 | 0.0062 | -0.0 | 20.57 | 20.95 | -183.6 | — | 187.1 | 184.1 | -79.7 | -182.5 |
| SoXR 32-bit | 7.0e-9 | 0.0078 | -0.0 | 20.58 | 20.94 | -207.0 | — | 212.5 | 207.2 | -77.7 | -190.8 |
| SoXR VHQ, steep | 1.3e-8 | 0.0017 | -0.0 | 21.75 | 21.82 | -181.6 | — | 190.0 | 183.9 | -91.1 | -183.0 |
| SoXR VHQ, intermediate | 1.4e-8 | 0.0062 | -0.0 | 20.57 | 20.95 | -183.7 | — | 187.1 | 183.9 | -8.7 | -26.0 |
| SoXR VHQ, minimum | 1.7e-8 | 0.0062 | -0.0 | 20.57 | 20.95 | -183.6 | — | 187.1 | 184.1 | -5.9 | -20.0 |

### Time domain, 44.1 kHz to 96 kHz

| Setting | Delay at 1 kHz (µs) | Delay spread 20 Hz–20 kHz (µs) | Impulse peak offset (µs) | Ringing before peak (ms) | Ringing after peak (ms) | Energy over 1 ms before peak (dB) | Streaming latency (ms) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| **Resonate Fast** | -0.0 | 0.0 | 0.0 | 0.43 | 0.43 | < -200 | 0.43 |
| **Resonate Balanced** | 0.0 | 0.0 | 0.0 | 1.06 | 1.06 | -94.2 | 1.07 |
| **Resonate High** | -0.0 | 0.0 | 0.0 | 2.65 | 2.65 | -44.3 | 6.26 |
| **Resonate Very high** | 0.0 | 0.0 | 0.0 | 5.67 | 5.67 | -34.9 | 10.75 |
| **Resonate High, minimum** | -15.0 | 58.7 | 0.0 | 0.09 | 4.84 | < -200 | 3.47 |
| **Resonate Very high, intermediate** | -12.6 | 38.7 | 0.0 | 4.14 | 8.23 | -60.9 | 12.56 |
| **Resonate Very high, minimum** | -16.8 | 77.5 | 0.0 | 0.11 | 10.88 | < -200 | 3.63 |
| SoXR QQ | -0.0 | 0.0 | 0.0 | 0.04 | 0.04 | < -200 | 0.05 |
| SoXR LQ | -0.0 | 0.0 | 0.0 | 0.47 | 0.47 | < -200 | 0.48 |
| SoXR MQ | -0.0 | 0.0 | 0.0 | 1.86 | 1.86 | -52.8 | 12.45 |
| SoXR 16-bit | -0.0 | 0.0 | 0.0 | 1.86 | 1.86 | -52.6 | 12.45 |
| SoXR HQ | 0.0 | 0.0 | 0.0 | 2.15 | 2.15 | -49.5 | 18.39 |
| SoXR 24-bit | 0.0 | 0.0 | 0.0 | 2.33 | 2.33 | -47.4 | 4.92 |
| SoXR VHQ | 0.0 | 0.0 | 0.0 | 2.57 | 2.57 | -45.7 | 8.41 |
| SoXR 32-bit | 0.0 | 0.0 | 0.0 | 2.74 | 2.74 | -44.5 | 18.44 |
| SoXR VHQ, steep | 0.0 | 0.0 | 0.0 | 11.77 | 11.77 | -31.8 | 24.83 |
| SoXR VHQ, intermediate | -13.4 | 38.0 | 0.0 | 1.98 | 3.68 | -76.0 | 34.35 |
| SoXR VHQ, minimum | -26.8 | 76.1 | -10.4 | 0.10 | 4.79 | < -200 | 5.40 |

### Frequency domain, 48 kHz to 96 kHz

| Setting | Ripple 20 Hz–10 kHz (dB) | Deviation 20 Hz–20 kHz (dB) | At 20 kHz (dB) | −0.1 dB (kHz) | −3 dB (kHz) | Worst alias or image (dB) | Worst landing ≤ 20 kHz (dB) | SNR at 1 kHz (dB) | Worst SNR 20 Hz–20 kHz (dB) | Null 20 Hz–20 kHz (dB) | Null 20 Hz–10 kHz (dB) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| **Resonate Fast** | 3.5e-3 | 0.4024 | -0.4 | 18.69 | 21.05 | -62.6 | — | 82.4 | 72.2 | -43.7 | -74.7 |
| **Resonate Balanced** | 9.1e-5 | 0.0002 | -0.0 | 21.52 | 22.29 | -89.7 | — | 153.3 | 102.4 | -104.7 | -104.8 |
| **Resonate High** | 1.1e-7 | < 0.0001 | -0.0 | 22.67 | 23.03 | -136.6 | — | 255.8 | 154.5 | -161.4 | -168.1 |
| **Resonate Very high** | 4.6e-10 | < 0.0001 | -0.0 | 23.23 | 23.41 | -185.8 | — | 282.9 | 204.7 | -209.8 | -211.0 |
| **Resonate High, minimum** | 1.2e-7 | < 0.0001 | -0.0 | 22.67 | 23.03 | -136.5 | — | 257.1 | 153.9 | -7.8 | -24.3 |
| **Resonate Very high, intermediate** | 2.4e-9 | < 0.0001 | -0.0 | 23.23 | 23.41 | -185.8 | — | 266.5 | 209.0 | -8.5 | -27.8 |
| **Resonate Very high, minimum** | 9.5e-9 | < 0.0001 | -0.0 | 23.23 | 23.41 | -185.8 | — | 263.5 | 205.4 | -7.9 | -21.8 |
| SoXR QQ | 2.1e-1 | 3.2258 | -3.2 | 7.74 | 19.55 | -6.0 | — | 109.3 | 10.2 | -21.8 | -42.3 |
| SoXR LQ | 1.7e-5 | 5.3675 | -5.4 | 16.78 | 19.17 | -103.9 | — | 130.1 | 109.3 | -23.6 | -125.8 |
| SoXR MQ | 1.3e-5 | < 0.0001 | 0.0 | 22.32 | 22.82 | -103.5 | — | 134.0 | 119.0 | -123.6 | -124.4 |
| SoXR 16-bit | 1.3e-5 | < 0.0001 | 0.0 | 22.32 | 22.82 | -103.5 | — | 134.0 | 119.0 | -123.6 | -124.4 |
| SoXR HQ | 2.2e-6 | < 0.0001 | -0.0 | 22.34 | 22.81 | -126.6 | — | 133.8 | 131.2 | -132.0 | -132.0 |
| SoXR 24-bit | 4.8e-8 | < 0.0001 | 0.0 | 22.37 | 22.80 | -152.4 | — | 175.5 | 166.1 | -170.4 | -171.9 |
| SoXR VHQ | 2.8e-9 | < 0.0001 | 0.0 | 22.39 | 22.80 | -177.1 | — | 210.7 | 191.3 | -194.5 | -196.3 |
| SoXR 32-bit | 8.8e-11 | < 0.0001 | 0.0 | 22.40 | 22.80 | -199.8 | — | 225.1 | 217.8 | -219.1 | -221.9 |
| SoXR VHQ, steep | 3.6e-10 | < 0.0001 | -0.0 | 23.67 | 23.75 | -176.3 | — | 225.1 | 199.7 | -209.0 | -211.5 |
| SoXR VHQ, intermediate | 2.7e-8 | < 0.0001 | -0.0 | 22.39 | 22.80 | -177.8 | — | 216.7 | 190.6 | -8.8 | -28.6 |
| SoXR VHQ, minimum | 1.0e-7 | < 0.0001 | -0.0 | 22.39 | 22.80 | -177.1 | — | 210.6 | 191.3 | -8.0 | -22.6 |

### Time domain, 48 kHz to 96 kHz

| Setting | Delay at 1 kHz (µs) | Delay spread 20 Hz–20 kHz (µs) | Impulse peak offset (µs) | Ringing before peak (ms) | Ringing after peak (ms) | Energy over 1 ms before peak (dB) | Streaming latency (ms) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| **Resonate Fast** | -0.0 | 0.0 | 0.0 | 0.39 | 0.39 | < -200 | 0.40 |
| **Resonate Balanced** | -0.0 | 0.0 | 0.0 | 0.97 | 0.97 | < -200 | 0.98 |
| **Resonate High** | 0.0 | 0.0 | 0.0 | 2.43 | 2.43 | -46.5 | 5.75 |
| **Resonate Very high** | 0.0 | 0.0 | 0.0 | 5.23 | 5.23 | -35.9 | 9.88 |
| **Resonate High, minimum** | -13.8 | 35.0 | 0.0 | 0.08 | 4.43 | < -200 | 3.19 |
| **Resonate Very high, intermediate** | -11.6 | 23.3 | 0.0 | 3.71 | 7.60 | -62.2 | 11.54 |
| **Resonate Very high, minimum** | -15.4 | 46.5 | 0.0 | 0.10 | 9.97 | < -200 | 3.33 |
| SoXR QQ | -0.0 | 0.0 | 0.0 | 0.03 | 0.03 | < -200 | 0.04 |
| SoXR LQ | -0.0 | 0.0 | 0.0 | 0.44 | 0.44 | < -200 | 3.96 |
| SoXR MQ | -0.0 | 0.0 | 0.0 | 1.65 | 1.65 | -57.7 | 8.33 |
| SoXR 16-bit | -0.0 | 0.0 | 0.0 | 1.65 | 1.65 | -57.7 | 8.33 |
| SoXR HQ | -0.0 | 0.0 | 0.0 | 1.91 | 1.91 | -53.6 | 13.25 |
| SoXR 24-bit | -0.0 | 0.0 | 0.0 | 2.12 | 2.12 | -50.8 | 16.67 |
| SoXR VHQ | -0.0 | 0.0 | 0.0 | 2.30 | 2.30 | -48.8 | 2.79 |
| SoXR 32-bit | 0.0 | 0.0 | 0.0 | 2.46 | 2.46 | -47.2 | 30.50 |
| SoXR VHQ, steep | 0.0 | 0.0 | 0.0 | 10.60 | 10.60 | -32.4 | 143.12 |
| SoXR VHQ, intermediate | -8.6 | 21.6 | 0.0 | 1.82 | 3.29 | -80.5 | 9.69 |
| SoXR VHQ, minimum | -27.6 | 43.3 | -10.4 | 0.09 | 4.26 | < -200 | 0.12 |

## Reproducing

```sh
cd soxr-comparison
cargo run --release -- --no-timing --plots ../docs/resampling
cargo run --release -- --timing-only
cargo run --release -- --fast-and-balanced
```

The first prints the frequency and time domain tables and writes the plots on every core, under
any load; the second prints the cost tables and wants an idle machine; the third searches Fast's
and Balanced's narrowed and lengthened kernels and measures and times them beside SoXR's nearest
recipes. All need libsoxr and its headers (`pkg-config --modversion soxr`).
