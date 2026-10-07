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
Leaf beside the vault (core, codec, dsp for `TruePeakMeter`/resampler, `rustfft`,
`rusty-chromaprint`); `cargo tree -p resonate-analysis` free of gpui, engine, library, `ureq`.
Engine re-exports its vocabulary; `Player::analyse` uses the player's own `Sources` (vaulted rows
analyse from their object). Library reaches `study` (enrichment) and `print` (moved files).

## One pass, many builders

- **`analyse` and `study` are one walk, differing in a type.** `walked` opens the decoder at the
  width the vault's `pcm_of` picks (integer samples keep low bits). `Drawing`: `Unseen` (`study`) /
  `Drawn` (envelope, spectrogram; `analyse`), so library-wide studies build nothing only a screen
  wants. DSD → F32 via its decimator, not DoP.
- **Non-number samples are silence**: `normalise` maps float NaN/infinity to 0 before any builder
  (else one rides the K-weighting history to track end). Bit reading keeps native samples.
- **`Watching` stops/follows a pass.** `Watch` is the window's; `EnrichProgress` implements it, so
  cancelling a lookup stops every study at its next block. `Error::Stopped` = no answer, not a
  failure.
- **`Doubling` bounds growth**: at the cap every column pair merges (unknown-length streams end in
  at most `ENVELOPE_COLUMNS` / `SPECTROGRAM_COLUMNS`).
- **Spectrum: Hann transform of every channel but LFE**, sized by rate (~11 Hz bins anywhere); bin =
  mean of channel powers. Not a mono mix (cancels opposite-phase stereo to silence → `NotJudged`;
  adds LFE rumble). Average skips windows under `SILENT_BELOW_DB`; `Spectrum::heard` = amount
  counted. Spectrogram axis linear (lowpass wall = horizontal line); `Spectrogram::painted` writes a
  `Raster` through the caller's `Ramp` (theme stays in the window).
- **Bits in use = trailing zeros of all samples ORed**, beside declared depth, never instead; float
  depth = `BITS_A_FLOAT_CARRIES`.
- **Loudness BS.1770, range EBU Tech 3342, DR the TT meter's.** K-weighting derived per stream rate;
  channel weight by placement (`resonate_codec::Speakers::placements`, WAVE order if source names
  none): LFE 0, side `SURROUND_WEIGHT`, rear `SURROUND_WEIGHT` only with no side channel, rest 1.0.
  Matches `ffmpeg -af ebur128` to the tenth, 5.1 included. True peak = `TruePeakMeter` (engine
  guard's own: study and playback agree about an over), stored `track_studies.true_peak`, handed to
  the player by `Library::hinting` (`audio.md`).
- **Print = Chromaprint, first `PRINTED_FOR` only**: `rusty-chromaprint`
  `Configuration::preset_test2` (libchromaprint/AcoustID default), URL-safe base64, unpadded.
  `resonate_core::Chromaprint` carries it with whole-track length (none declared: decode to end);
  lookups ask with the track's known length, else the print's. Printer refusing a stream: debug
  record, `print: None`, never a failed analysis.
- **A clip is signed, never printed**: AcoustID filters by a whole recording's `duration`, matches
  from its start. Listen asks Shazam and AudD; AcoustID only a file printed whole (lookup, Analysis
  pane). `signature_of` writes Shazam's signature from the documented format, not any client's
  source: mono, 16 kHz via `Resampler` at `Balanced`, at most `SIGNED_SECONDS_AT_MOST` from the
  middle, as a `data:audio/vnd.shazam.sig;base64,` URI. A steady tone signs as nothing.

## What the window drew is kept

**Per-file, forgotten when the file changes.** `KeptAnalyses`: one file each in
`$XDG_CACHE_HOME/resonate/analyses` (given to the window's `Player` by `Player::keeping_analyses`),
named by FNV-1a of path, span, size, mtime; where only a vault object serves the row, path and span
(`NO_FILE_STANDS`: no re-decode per visit; a file arriving at the path takes another name).
`Player::analyse` asks it before decoding, keeps what it decoded. Another `JUDGED_UNDER` stamp reads
as nothing; judgement unwritten (`read` runs `judged` over kept spectrum and levels). Reader trusts
no shape: envelope with no lanes, over `ENVELOPE_LANES`, or columns of no frames → nothing.
Unreadable files deleted; writes staged + renamed; past `KEPT_BYTES_AT_MOST` least lately used go
first. Cache: failure = debug record + decode. Headless commands keep nothing.

## The verdict

`verdict.rs` is pure: `Weighed` (codec, rate, declared depth, float flag, spectrum, levels) →
`Judgement`: `Verdict` (`Genuine`, `Suspect`, `Fake`, `Lossy`, `NotJudged`) + typed `Finding`s.
`JUDGED_UNDER` names the heuristic; bump by hand when its answer or a stored measure could change
(stored studies, kept analyses are weighed against it).

- **Wall = steep drop to a floor that stays.** 100 Hz bands up to `TOP_GUARD`. Edge is a wall where
  mean of bands 1 kHz..200 Hz below is `WALL_DB` over mean from 200 Hz above to top, *and*
  `WALL_DB` less `WALL_FLOOR_SLACK_DB` over the loudest of those (gentle slope isn't one). Cutoff =
  steepest edge of the highest run of walled edges (where content ends, not first thins).
- **Looked for in the average and in what a quarter of windows stay under**: loud masters decoded
  from lossy clip, splatter fills the average above the lowpass, the wall stays in every unclipped
  window. `Spectrum::typical` = each band's `TYPICAL_QUANTILE` off a per-band histogram; `judged`
  runs `wall_in` on both, `lower_of` takes the lower
  (`a_transcode_splattered_above_its_wall_by_clipping_is_still_fake`).
- **Position = meaning.** At/under `LOSSY_CEILING_HZ`: lossy cut → `Fake` + `LossyGuess` (LAME's
  lowpass table). Up to `SUSPECT_CEILING_HZ`: `Suspect` (high-bitrate encode ≈ low anti-alias
  filter). Above: anti-alias filter. Above 48 kHz, weigh against rates it could be upsampled from
  (at most half its own): wall within `UPSAMPLE_SLACK_HZ` of one's Nyquist = `Upsampled` = `Fake`.
  Under 44.1 kHz ceilings are unreachable: wall at/above `ANTI_ALIAS_EDGE` of Nyquist = converter's
  (`Genuine`, no lossy guess); lower → lossy rule (`Fake` + guess); no wall: content reaching the
  edge `Genuine`, else `NotJudged`
  (`a_lossless_file_at_a_low_rate_is_judged_against_its_own_anti_alias_edge`).
- **No wall is not a pass.** Content past `SUSPECT_CEILING_HZ` (hi-res: past where the highest
  possible source rate would end) within `CONTENT_WITHIN_DB` of the 1-4 kHz level: `Genuine`; fading
  below that: `NotJudged` (old/band-limited masters fade like transcodes).
- **Unjudgeable stays unjudged.** Lossy codec: `Lossy` whatever the spectrum, wall still reported.
  DSD: `NotJudged` (decimator draws its own wall). Under `HEARD_AT_LEAST` of loud audio: nothing
  judged. Padding judged without the spectrum → `Fake`. Identical channels, clipped runs, DC offset:
  findings only.
- **Walls and padding only.** AAC 256+ has no findable lowpass; no other statistic tried (band
  levels, holes, side channel, MDCT coefficient counts) separated it from its source. Opus lowpasses
  at 20 kHz at any bitrate: an Opus transcode is `Suspect`.

## Studies in the catalog

- **`track_studies`: one row per track, trigger-forgotten on a changed file.** Holds verdict +
  basis, levels, loudness, print, recognition (`recognised`, `heard_as`, score, title, artist,
  `Agreement`). `track_studies_forget_a_changed_file` deletes the row when an update moves size,
  mtime or span (trigger, not upsert clause: covers every writer). Tag runs are the one writer known
  not to touch audio: `files_retagged` holds a written file's `track_studies` and `unstudied` rows
  across its follow (`KeptAcross`, which holds its kept lyrics alike: `library.md`), puts them back.
- **A failed study is kept as failed.** `unstudied` holds the id under the failing `JUDGED_UNDER`;
  `to_study` and `Library::will_not_study` skip it until `unstudied_forget_a_changed_file` fires,
  `JUDGED_UNDER` moves, or `refresh` re-asks. Stopped studies and out-of-reach files
  (`Error::is_out_of_reach` ← `codec::Error::is_out_of_reach`: unmounted drive, downed share) leave
  no mark.
- **Enrichment studies every track beside the pass.** `Studies`: `available_parallelism / 2` threads
  (min 1) named `resonate-study-<n>`, one shared counter, started before the pass, joined after the
  pictures. `to_study` = no study, older `JUDGED_UNDER`, or unrecognised print (only where something
  can recognise; `refresh` re-asks from the kept print, no second decode). List says only *whether*
  a print is kept (`ToStudy::print_held`); worker reads it on taking the row
  (`Library::print_held`): prints never all in memory. `EnrichOptions::studies` (`study` key) leaves
  the pool unstarted; the fingerprint route still studies the one track it recognises.
- **Studies read what the player reads**: `Library::sources` carries `VaultFiles` where a vault is
  held (vaulted row, file gone: studied from its object).
- **Once per track, first arrival wins** (`Claims`, shared by pool and pass): workers skip claimed
  tracks; the fingerprint route waits, then reads what landed.
- **Recognition is weighed against the row's name.** `enrich::agreement` takes matches at the
  library's `HEARD_AT_LEAST` (80): none `Unheard`; file named no title `Unnamed`; match on the row's
  recording id (own `tracks.mbid` or paired release row's) or title+credit via `same_name` and
  `same_credit` `Agrees`; else `Disagrees`. Refused lookup stamps nothing (re-asked), but
  `RECOGNITIONS_REFUSED_BEFORE_GIVING_UP` in a row (`EnrichProgress`, across workers, reset by any
  answer) ends print asking; studying continues.
- **`Route::Fingerprint` reads the stored recognition first**, studying via the same claim if none
  kept; strict-score match = `Certainty::Nearly`: audio fills a name the file never gave, never
  overwrites one it did.
- **Heard name becomes the row's only when a listener says so** (a score is no reason to rename a
  tagged file). `Library::take_what_was_heard` (`enriched::land_what_was_heard`), one transaction:
  `heard_title` → title, `heard_artist` → artist (repoints `artist_id` via
  `store::artist_named_in`), `heard_as` → `tracks.mbid`, *replacing* the file's recording id (tags
  that misname a file misname its tag ids). Other claims go too (ISRC, release-track id, release
  title, numbers, `artist_mbid` where a heard artist is billed) plus any `release_tracks` pairing.
  Then rematch the album: seat the track on the release's row for that recording, or leave it
  unpaired and list the row it left missing. Stamps `answered`, clears `asked` (rescan keeps it all,
  `library.md`), marks the study `Agrees`. Analysis pane: *Take this name* where audio disagrees
  with the file or the file names nothing (`AnalysisModel::take_the_name`);
  `resonate studies --take <FILE>` is the same gesture.
- **`is:fake`, `is:suspect`, `is:misnamed` are `Shape`s**: an `EXISTS` over `track_studies`.

## Recognition

- **Seam `Fingerprints`; a `Sounded` carries the print.** Library never prints for the service,
  service never decodes. `Fingerprinters::recognise` offers each printer the print in turn until one
  matches; answers a `Recognition` (matches, whether a printer refused).
- **`AcoustId` = `resonate-online`'s printer, listener's registered key.** `Host::AcoustId` paced at
  `ACOUSTID_INTERVAL` (three a second). POST of a form to `/lookup`, gzipped (`Content-Encoding:
  gzip`; prints overflow query strings; `Posted::packed_form` falls back to plain). Each result's
  recordings → `RecordingMatch`es scored by the result's score; repeats keep the best, untitled
  dropped. `acoustid-key` empty by default; `online::fingerprinters` registers `AcoustId` only once
  it is set and `online` is on (`ByEar` sits behind it, `online.md`).

## The command line and the pane

- **`resonate analyse` takes a cut or a file, read as the queue reads it**: URI via
  `from_uri_within` (`#frames=` names the same cut), `.cue` via `read_cue_media` + `--track`. Own
  errors: sheet without `--track` (`SheetWithoutATrack`), number the sheet lacks
  (`NoSuchSheetTrack`), `--track` beside a non-sheet (`TrackOutsideASheet`).
- **`Pane::Analysis` follows the playing row, analyses only while in front.**
  `AnalysisModel::follow` runs from the render; a new row stops the old `Watch`, starts
  `Player::analyse` on the background executor; `ANALYSES_KEPT` rows held per run. Condensing once
  on landing (`WAVEFORM_COLUMNS`, `SPECTRUM_COLUMNS`); spectrogram painted once per row and ramp in
  the background (a frame computes nothing).
- **What the pane learns goes to the catalog.** `settled` writes the study where the catalog holds
  the row and none is kept, never over one (would drop a landed recognition), recognising via
  `Library::recognise`. A row no scan has seen is recognised through the registry directly, no
  agreement. The recognition card leads where the audio isn't the song the file names.
