# Performance audit

These are code-backed optimization candidates from a read-only audit. None has been implemented or measured. The order reflects likely payoff and confidence, not benchmark results. Preserve the behavior and invariants in `CLAUDE.md` and `.claude/rules/` when evaluating one.

## Repeated background work

- **MPRIS active playlist lookup.** The 200 ms [snapshot poll](../crates/resonate-mpris/src/service.rs) calls `collect`, which calls `playlists.playing()` even when the playlist revision is unchanged. The binary implementation resolves the active playlist through catalog queries, including aggregation of playlist entries. Cache the answer against the playing queue stamp and playlist revision while preserving active-playlist signals. Measure database calls and poll time with a large playlist and a saved query.
- **MPRIS playing-track history.** Each snapshot scans the queue and calls `Host::heard`; the desktop host reads and decodes the current track row through `Library::track_at` in [mpris.rs](../crates/resonate/src/mpris.rs). Cache the answer with an invalidation path for changes to play statistics. Measure catalog reads and poll time during playback.
- **MPRIS unchanged queue comparisons.** [Track-list change detection](../crates/resonate-mpris/src/tracklist.rs) scans the queue's common prefix and suffix, then [signal publication](../crates/resonate-mpris/src/service.rs) compares its IDs again every 200 ms. The engine retains the same queue `Arc` until its revision changes, so pointer equality can skip unchanged scans while keeping the current content diff for changed queues. Measure poll time with long queues.

## Scanning and importing

- **Metadata calls for irrelevant files.** [The directory walker](../crates/resonate-library/src/scan.rs) calls `fs::metadata` before rejecting ordinary files that are neither audio nor cue sheets. Filter those files using `file_type` and extension first. Preserve target metadata and cycle handling for symlinks. Measure scan time and `statx`/`newfstatat` counts on trees with unrelated files, including network storage.
- **Cue-sheet path lookup.** [Cue resolution](../crates/resonate-library/src/scan.rs) linearly searches the directory's audio vector for each `FILE` reference, making the work proportional to cue references times audio files. A temporary path index could reduce lookups while retaining the existing ordered pass and claim behavior. Measure on cue-heavy directories; small directories may not benefit.
- **Kept-form decoder reopen.** [Vault import](../crates/resonate-vault/src/vault.rs) opens a decoder before choosing `Form::Kept`, then opens another for source PCM validation in `kept_whole`. The direct kept branch could reuse the first decoder. The `NoSmaller` fallback has consumed it and still needs a new one. Preserve raw copying, staged readback and PCM comparison. Measure decoder opens and import time for kept files.
- **Provider attempt threads.** [Provider polling](../crates/resonate-providers/src/provider.rs) starts a thread for each due want/provider attempt. Measure thread starts and elapsed time on large inbox polls before changing the timeout and late-delivery design.
- **Stream chunk allocations.** [Delivery pumping](../crates/resonate-library/src/supply.rs) allocates a fresh 64 KiB vector per chunk. Measure allocation and CPU share for large streams before changing channel ownership or backpressure behavior.

## Playback and analysis

- **Resampler deinterleave.** [Resampler input append](../crates/resonate-dsp/src/resample.rs) walks interleaved input once per channel to fill planar history on every converted playback block. Compare its current loop with a frame-major scatter on the existing DSP stage benchmark, at stereo and multichannel rates. The memory traffic is similar, so a gain must be measured.
- **Resampler frame dispatch.** [Output emission](../crates/resonate-dsp/src/resample.rs) matches tabulated versus interpolated taps inside the produced-frame loop. Benchmark before splitting the loops; the compiler may already hoist the stable choice.
- **Integer level pass.** [Stereo integer analysis](../crates/resonate-analysis/src/levels.rs) reads samples once for the OR mask and again for left/right identity. A single pass could compute both. Measure whole-track analysis CPU time on integer stereo sources.
- **DoP packing.** [PipeWire's in-place packer](../crates/resonate-pipewire/src/process.rs) performs checked lookups and conversions per word. A chunk-based loop might reduce overhead. Measure at DoP quantum sizes and preserve the real-time no-allocation contract.
- **Repeated clip downmix.** [Shazam](../crates/resonate-online/src/shazam.rs) and [AudD](../crates/resonate-online/src/audd.rs) each call `Clip::mono` when both recognizers are tried, scanning and allocating for the same clip twice. Share the mono data across an attempt if preparation benchmarks show a useful gain without slowing a one-provider run.

## Interface and search

- **Visible queue row clones.** [Queue rendering](../crates/resonate-ui/src/views/queue.rs) calls `track_of` for each visible row, and a [cache hit](../crates/resonate-ui/src/models.rs) deep-clones a `Track` with owned strings. A shared or borrowed read path could remove these per-redraw copies. Measure allocations and frame build time while scrolling and during playback.
- **Search highlight folding.** [Search::lit](../crates/resonate-library/src/search.rs) tokenizes displayed text and folds the same query pieces for every highlighted cell; [browser rows](../crates/resonate-ui/src/views/browser.rs) call it separately for title and artist. Pre-folding query pieces may help while retaining original byte ranges for highlights. Measure Unicode searches on visible listings. The empty-query fast path is already deliberate.
- **Analysis plot clones.** [The analysis pane](../crates/resonate-ui/src/views/analysis.rs) clones prepared waveform lanes and the 480-point spectrum into canvas closures on rebuild. The plot already has shared ownership, so measure whether sharing these buffers reduces frame build cost on multichannel tracks.
- **Inspector graph vectors.** [Graph drawing](../crates/resonate-ui/src/views/inspector.rs) converts its already-cached, 240-point bitrate series into new share and paint vectors. This is bounded micro work; measure allocation rate with the inspector visible before changing it.
- **AutoEq suggestion allocations.** [Catalogue suggestions](../crates/resonate-eq/src/catalogue.rs) allocate a temporary tail vector for each candidate label during a scan of roughly 8,850 entries. A slice could avoid those allocations. Measure suggestion latency and allocations against the full index, and verify identical ranking.

## Lower-confidence query work

- [Aggregate catalog search](../crates/resonate-library/src/db.rs) runs track, album and artist queries for the same text, parsing it separately for each. Parsing is intentionally uncached under the current [library rule](../.claude/rules/library.md); measure parser time against the three SQLite queries before considering reuse.
- [List-query execution](../crates/resonate-library/src/db.rs) prepares statements on each call. Repeated identical shapes might benefit from `prepare_cached`, but dynamic SQL could churn the cache. Benchmark representative UI queries before changing preparation.
