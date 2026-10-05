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

`resonate-analysis` decodes a track whole and says what it is: its envelope, spectrum, whether its
container's lossless claim holds, loudness and print. A leaf beside the vault (core, codec, dsp for
the true-peak meter and the resampler, `rustfft`, `rusty-chromaprint`); `cargo tree -p
resonate-analysis` stays free of gpui, the engine, the library and `ureq`. The engine re-exports its
vocabulary and answers `Player::analyse` through the player's own `Sources` (so a vaulted row is
analysed out of its object); the library reaches `study` for the enrichment.

## One pass, many builders

- **`analyse` and `study` are one walk, differing in a type.** `walked` opens the decoder at the
  width the vault's `pcm_of` would choose, so integer samples keep their low bits. `Drawing` is the
  seam: `Unseen` for `study`, `Drawn` (envelope and spectrogram) for `analyse`, so a library-wide
  study builds nothing only a screen wants. DSD is decoded to F32 through its decimator, not DoP.
- **A sample that is not a number is weighed as silence.** `normalise` turns a float file's NaN or
  infinity into 0 before any builder, since one would ride the K-weighting history to the end of the
  track. The bit reading keeps native samples.
- **`Watching` is how a caller stops and follows a pass.** `Watch` is the window's;
  `EnrichProgress` implements the same trait, so cancelling a lookup stops every study at its next
  block. A stopped pass is `Error::Stopped`, which callers read as no answer, not a failure.
- **What would grow without bound is held in `Doubling`**: when columns reach the cap every pair
  merges, so a stream of unknown length ends in at most `ENVELOPE_COLUMNS` / `SPECTROGRAM_COLUMNS`.
- **The spectrum is a Hann-windowed transform of every channel but the LFE**, sized by rate (a bin
  is ~11 Hz anywhere); each bin is the mean of the channels' powers. Not a mono mix: a mix cancels a
  stereo pair in opposite phase to silence (`NotJudged`) and sums the LFE's rumble in. The average
  skips windows quieter than `SILENT_BELOW_DB`; `Spectrum::heard` is how much counted. The
  spectrogram's axis is linear, where a lowpass wall is a horizontal line; `Spectrogram::painted`
  writes a `Raster` through a caller's `Ramp`, so the theme stays in the window.
- **The bits in use are the trailing zeros of every sample ORed together**, weighed beside the
  declared depth, never instead of it; a float file's depth is `BITS_A_FLOAT_CARRIES`.
- **Loudness is BS.1770, range is EBU Tech 3342, DR is the TT meter's.** The K-weighting is derived
  for the stream's own rate; a channel is weighed by where its speaker sits
  (`resonate_codec::Speakers::placements`, WAVE order where the source names none): LFE nothing,
  side and rear surround `SURROUND_WEIGHT`, the rest 1.0. The reading matches `ffmpeg -af ebur128`
  to the tenth, 5.1 included. The true peak is `resonate-dsp`'s `TruePeakMeter` (the engine guard's
  own, so study and playback cannot disagree about an over), stored as `track_studies.true_peak` and
  handed to the player by `Library::hinting` (`audio.md`).
- **The print is Chromaprint's own algorithm, first `PRINTED_FOR` only**: `rusty-chromaprint` under
  `Configuration::preset_test2` (what libchromaprint and AcoustID default to), URL-safe base64 with
  no padding. `resonate_core::Chromaprint` carries it with the whole track's length, the duration a
  lookup is asked with; a container declaring no length is decoded to its end for that. A stream the
  printer refuses to start is a debug record and `print: None`, never a failed analysis.
- **A clip is signed, never printed.** AcoustID filters by a whole recording's `duration` and
  matches prints from a recording's start; a clip has neither. Listen asks Shazam and AudD;
  AcoustID is asked only about a file printed whole, by the lookup and the Analysis pane.
  `signature_of` writes Shazam's signature from the documented format, not any client's source:
  mono, 16 kHz through `Resampler` at `Balanced`, at most `SIGNED_SECONDS_AT_MOST` from the middle,
  handed on as a `data:audio/vnd.shazam.sig;base64,` URI. A steady tone signs as nothing.

## What the window drew is kept

**An analysis is kept against the file it was taken of, and a changed file forgets it.**
`KeptAnalyses` is a folder of one file per analysis (`$XDG_CACHE_HOME/resonate/analyses`, given to
the window's `Player` through `Player::keeping_analyses`), named by an FNV-1a of path, span, size and
modification time — or, where the file is gone and only a vault object serves the row, of path and span
alone (`NO_FILE_STANDS`), so a vaulted track's analysis is not decoded again on every visit and a file
arriving at the path takes another name. `Player::analyse` asks it before decoding and keeps what it decoded. A file
stamped under another `JUDGED_UNDER` reads as nothing; the judgement is not written, `read` runs
`judged` over the kept spectrum and levels. The reader trusts none of its own shape: an envelope
with no lanes, more than `ENVELOPE_LANES`, or columns of no frames reads as nothing. A file that
will not read back is deleted, writes land by staged name and rename, and past `KEPT_BYTES_AT_MOST`
the least lately used go first. It is a cache: every failure is a debug record and a decode. The
headless commands keep nothing.

## The verdict

`verdict.rs` is pure: a `Weighed` (codec, rate, declared depth, spectrum, levels) in, a `Judgement`
out: a `Verdict` (`Genuine`, `Suspect`, `Fake`, `Lossy`, `NotJudged`) with typed `Finding`s.
`JUDGED_UNDER` names the heuristic and is bumped by hand whenever its answer, or a measure a study
keeps, could change, since stored studies and kept analyses are weighed against it.

- **A wall is a steep drop to a floor that stays there.** The spectrum is read in 100 Hz bands up to
  `TOP_GUARD`. An edge is a wall where the mean of the bands a kilohertz to 200 Hz below it stands
  `WALL_DB` over the mean of everything from 200 Hz above to the top, *and* `WALL_DB` less
  `WALL_FLOOR_SLACK_DB` over the loudest of those, which keeps a gentle slope from reading as a wall.
  The steepest edge of the highest run of walled edges is the cutoff: a wall is where content ends,
  not where it first thins.
- **A wall is looked for twice: in the average and in what a quarter of the windows stay under.** A
  loud master decoded from a lossy encode clips, and the splatter fills the average above the
  encoder's lowpass while the wall is in every unclipped window. `Spectrum::typical` is each band's
  `TYPICAL_QUANTILE` off a per-band histogram, `judged` runs `wall_in` over both and `lower_of` takes
  the lower wall (`a_transcode_splattered_above_its_wall_by_clipping_is_still_fake`).
- **Where the wall stands is what it means.** At or under `LOSSY_CEILING_HZ` it is where a lossy
  encoder cuts: `Fake`, with a `LossyGuess` from LAME's lowpass table; up to `SUSPECT_CEILING_HZ`
  `Suspect` (a high-bitrate encode and a low anti-alias filter cannot be told apart); above, an
  anti-alias filter. Above 48 kHz the stream is weighed against rates it could have been upsampled
  from, only those at most half its own; a wall within `UPSAMPLE_SLACK_HZ` of one's Nyquist is
  `Upsampled`, which is `Fake`. Under 44.1 kHz neither ceiling is reachable, so a wall at or above
  `ANTI_ALIAS_EDGE` of Nyquist is the converter's (`Genuine`) and below it `Suspect`, never a lossy
  guess.
- **No wall is not a pass.** Content reaching past `SUSPECT_CEILING_HZ` (hi-res: past where the
  highest possible source rate would end) within `CONTENT_WITHIN_DB` of the 1-4 kHz level is
  `Genuine`; fading below that with no wall is `NotJudged`, since old and band-limited masters fade
  as a transcode does.
- **What the spectrum cannot judge is not judged.** A lossy codec is `Lossy` whatever its spectrum,
  still reporting the wall; DSD is `NotJudged`, its decimator drawing a wall of its own; under the
  verdict's `HEARD_AT_LEAST` of loud audio nothing is judged. Padding is judged without the
  spectrum and makes a file `Fake`. Identical channels, clipped runs and DC offset are findings that
  change no verdict.
- **The verdict reads walls and padding alone.** AAC 256 and above has no lowpass a wall can find,
  and no other statistic tried (band levels, holes, side channel, MDCT coefficient counts)
  separated it from its source. Opus lowpasses at 20 kHz whatever the bitrate, so it lands
  `Suspect`.

## Studies in the catalog

- **`track_studies` is one row per track, and a changed file forgets its row by trigger.** It holds
  the verdict and its basis, levels, loudness, the print, and the recognition (`recognised`,
  `heard_as`, score, title, artist, an `Agreement`). `track_studies_forget_a_changed_file` deletes
  the row wherever an update moves size, mtime or span; a trigger rather than an upsert clause so
  every writer is covered. A tag run is the one writer known not to touch the audio, so
  `files_retagged` holds a written file's `track_studies` and `unstudied` rows across its follow
  (`StudiesKept`) and puts them back.
- **A study that fails is kept as failed.** `unstudied` holds the id under the `JUDGED_UNDER` it
  failed at; `to_study` and `Library::will_not_study` skip it until
  `unstudied_forget_a_changed_file` fires, `JUDGED_UNDER` moves, or a `refresh` asks again. A stopped
  study and a file out of reach (`Error::is_out_of_reach`, from `codec::Error::is_out_of_reach`: an
  unmounted drive, a share that was down) leave no mark.
- **The enrichment studies every track beside the pass.** `Studies` is a pool of
  `available_parallelism / 2` threads (at least one) named `resonate-study-<n>`, drawing off one
  counter, started before the pass and joined after the pictures. `to_study` is every track with no
  study, a study under an older `JUDGED_UNDER`, or a print not yet recognised (only where something
  can recognise it; `refresh` asks the service again from the kept print rather than decoding twice). The list says only *whether* a print is kept (`ToStudy::print_held`); the worker reads
  the print when it takes the row (`Library::print_held`), so a large library's prints are never all in memory.
  `EnrichOptions::studies` (the `study` key) leaves the pool unstarted; the fingerprint route still
  studies the one track it must recognise.
- **A study reads what the player reads.** `Library::sources` carries `VaultFiles` where a vault is
  held, so a vaulted row whose file has gone is studied from its object.
- **A track is studied once, by whoever reaches it first** (`Claims`, shared by pool and pass): a
  worker passes over a claimed track; the fingerprint route waits and reads what landed.
- **Recognition is weighed against what the row is called.** `enrich::agreement` takes matches at
  the library's `HEARD_AT_LEAST` (80): none is `Unheard`; a row whose file named no title `Unnamed`;
  a match on the row's recording id (its own `tracks.mbid` or its paired release row's) or on title
  and credit through `same_name` and `same_credit` `Agrees`; anything else `Disagrees`. A refused
  lookup stamps nothing, so it is asked again, but after `RECOGNITIONS_REFUSED_BEFORE_GIVING_UP` in
  a row (counted by `EnrichProgress` across workers, reset by any answer) the pass keeps studying
  and asks no more prints.
- **`Route::Fingerprint` reads the stored recognition before it asks**, studying the track through
  the same claim if none is kept, and takes a match under the strict score as `Certainty::Nearly`, so
  audio fills a name the file never gave and never overwrites one it did.
- **What the audio was heard as becomes the row's name only when a listener says so.**
  `Library::take_what_was_heard` (`enriched::land_what_was_heard`) is one transaction: `heard_title`
  as title, `heard_artist` as artist (repointing `artist_id` through `store::artist_named_in`) and
  `heard_as` as `tracks.mbid`, *replacing* the file's recording id, since a file misnamed by its tags
  is misnamed by its tag ids too. The rest of the file's word about which song it is goes with it
  (ISRC, release-track id, release title, numbers, the artist id where a heard artist is billed), as
  does any `release_tracks` pairing. It then rematches the album: the track is seated on the
  release's row for that recording, or stands unpaired and the row it left is listed missing. It
  stamps `answered` and clears `asked`, so a rescan keeps it all (`library.md`), and marks the study
  `Agrees`. A gesture rather than a rule of the pass, because a score is no reason to rename a file
  somebody tagged. The Analysis pane offers it as *Take this name* where the audio disagrees with the
  file or the file names nothing (`AnalysisModel::take_the_name`); `resonate studies --take <FILE>`
  is the same gesture.
- **`is:fake`, `is:suspect` and `is:misnamed` are `Shape`s**, an `EXISTS` over `track_studies`.

## Recognition

- **The seam is `Fingerprints`, and a `Sounded` carries the print.** The library never prints for the
  service and the service never decodes; `Fingerprinters::recognise` hands every printer the print in
  turn and answers a `Recognition` (the matches and whether a printer refused).
- **`AcoustId` is `resonate-online`'s printer, asking with a key the listener registered.**
  `Host::AcoustId` is paced at `ACOUSTID_INTERVAL` (the service's three a second). The lookup is a
  POST to `/lookup` of a form, gzipped under `Content-Encoding: gzip` because a print is too long
  for a query string; `Posted::packed_form` falls back to the plain form. Each result's recordings
  become `RecordingMatch`es scored by the result's score, a recording answered twice kept at its
  best, one with no title dropped. `acoustid-key` is empty by default and `online::fingerprinters`
  registers nothing but the stub until it is set and `online` is on.

## The command line and the pane

- **`resonate analyse` takes a cut as well as a file, read the way the queue reads it**: a URI
  through `from_uri_within` (a `#frames=` names the same cut), a `.cue` through `read_cue_media` and
  `--track`. A sheet with no `--track`, a number the sheet lacks and a `--track` beside a non-sheet
  are each an error of their own.
- **`Pane::Analysis` follows the playing row and analyses only while in front.**
  `AnalysisModel::follow` is called from the render; a new row stops the old `Watch` and starts
  `Player::analyse` on the background executor, and `ANALYSES_KEPT` rows are held for the run.
  Condensing is done once when the analysis lands (`WAVEFORM_COLUMNS`, `SPECTRUM_COLUMNS`) and the
  spectrogram is painted once per row and ramp in the background, so a frame computes nothing.
- **What the pane learns it hands to the catalog.** `settled` writes the study where the catalog
  holds the row and none is kept, never over one (which would drop a recognition the lookup landed),
  and recognises through `Library::recognise`. A row no scan has seen is recognised through the
  registry directly and carries no agreement. The recognition card leads where the audio is not the
  song the file names.
