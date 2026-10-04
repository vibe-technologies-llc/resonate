---
paths:
  - "crates/resonate-lyrics/**/*.rs"
  - "crates/resonate-ui/src/lyrics.rs"
  - "crates/resonate-ui/src/views/lyrics.rs"
  - "crates/resonate-codec/src/sylt.rs"
  - "crates/resonate-online/src/lrclib.rs"
  - "crates/resonate-library/src/sung.rs"
---

# Lyrics

`resonate-lyrics` carries the vocabulary and the provider seam; `resonate-ui` draws it. The crate
takes no dependency on gpui, the engine or the library, so all of it is tested without a window.

## The vocabulary

- **A `Lyrics` is a flat list of `LyricLine`s, `Synced` or `Unsynced`.** A line carries an optional
  moment and a `Voice` (one by default, two where a timed LRC line names it). `Wanted` is the track a
  provider is asked about, with the file's own text as `Wanted::carried`.
- **`Lyrics::line_at` is what the pane centres on, `voices_in_play` what it lights, and they part
  company.** `line_at` is the last line whose moment has passed, so a long instrumental holds its
  place. Each voice stays lit up to its next line or `LIT_AT_MOST`, so one singer entering does not
  put out the other; with neither active the set dims while the pane stays put.
- **A `Credits` is what a sheet claims about its own making**, not which track it is, so an embedded
  set carries credits though it is checked against nothing.
- **A line may carry its words, each with its own moment, and the end the sheet gave it.**
  `LyricLine::worded` makes the line's text the words' joining, so a word is a byte range of the line
  and no second copy can disagree. `Lyrics::detail` (`Unsynced`, `Lines`, `Words`) is the order
  `Lyricists::find` ranks by and the pane's badge. `LyricLine::sweep_at` gives the `Sweep` of bytes
  sung and the `Singing` word with how far through it the transport is; `Lyrics::within` shifts a
  line onto a cue row's clock.
- **The pane sweeps a word-timed line word by word**, only for a line `voices_in_play` names: unsung
  words at `UNSUNG_SHARE` of the way to the lit colour, the word being sung *wiped* letter by letter,
  as highlight runs over one `StyledText` so the line wraps as it would unswept.
- **A line goes out once sung, and a blank line is a pause.** A synced set carries when a line
  starts, not when it ends, so `span_of` guesses: `sung_for` is `SUNG_BEFORE_THE_WORDS` plus
  `SUNG_PER_LETTER` a letter, held between `SUNG_AT_LEAST` and `LIT_AT_MOST`, and a line goes out
  there only where its voice's next line is at least `A_BREATH_AT_LEAST` further on; otherwise it
  stays lit until that line, so a verse never flickers. **Across two voices the same holds for
  whichever line comes next** (`answered`), or the pane stands empty with no dots between singers
  (`a_line_waits_for_the_other_voice_rather_than_leaving_less_than_a_breath_unlit`). **A line whose
  sheet gave it an end goes out exactly there.** A timed blank line ends the line before it and is
  never in play, so the pause counts down to the next written line. `has_ended` is the reading past
  the last line.
- **`waiting_at` is what a flat set can say about a gap, `breathes_before` the same read ahead.**
  Only where nothing is in play: which line the wait is for and how far through it the transport is,
  counted from the later of the last line of each voice to go out
  (`a_wait_counts_from_whichever_voice_went_out_last`). Both live here as the one arithmetic that must
  agree with `LIT_AT_MOST`; the pane gives a line its room once, at layout.

## Providers

- **Lyrics come out of the file, and one LRC reader is the whole of how.** `Sidecar` reads an `.lrc`
  or `.txt` beside a local file or in a lyrics folder next to it; `Embedded` reads `Wanted::carried`,
  which is `TagSet::lyrics` (`USLT`, `LYRICS`, `UNSYNCEDLYRICS`, `UNSYNCED LYRICS`, `©lyr`). ID3's
  `SYLT` reaches symphonia as a raw frame and `resonate-codec`'s `sylt.rs` writes it out as LRC
  (syllable-timed entries become a word-timed line), outranking an unsynchronised frame beside it. A
  frame timed in MPEG frames, or whose content type is not lyrics or a transcription, is left
  unread; `MOST_SYLLABLES` bounds the read; of several language frames the first stands. A sidecar outranks the
  file's own as the deliberate one. `read_lyrics(source, text)` is the reader's one public door, used
  by `Embedded` and `Lrclib`, so tag text and service text are read under the same bounds.
- **A provider reaching a network lives in `resonate-online`, what it keeps in the library.** `Lrclib`
  is a `LyricProvider` appended by the binary through `Lyricists::and` where `online` is on
  (`online.md`). A cue row is a span of a file, so `Wanted::span` keys the catalog's `lyrics_kept` by
  `(path, span_start)` as `tracks` is. The kept row is read before any request and a miss is
  remembered (`MISSED_AGAIN_AFTER`, `sung.rs`). The Lyricsfile reader (`read_lyricsfile`) is this
  crate's, so a build without `online` still reads the sidecar.
- **`Lyricists` walks the providers, and a refusal is not an answer.** `Lyricists::local` registers
  the two behind `Unsourced`, the one source `has_a_source` discounts, so a build with neither says no
  source is configured rather than that the track has no words. A refusing provider is logged and the
  walk continues; a refusal is reported only where nothing else answered. **The walk hands over the
  most finely timed set, not the first**: an answer replaces the one held only where its `Detail` is
  higher, so order breaks a tie; a word-timed set ends the walk.
  **The pane's lookup is `LyricsModel::_find`, one task**: a row leaving the queue (no `Wanted`) drops
  it, so a lookup still running cannot land the last track's words over *none*.
- **A sidecar is checked against its track; an embedded set never is.** `lrc::read` answers a `Sheet`
  with a `Declared` of what its `[ti:]`, `[ar:]` and `[length:]` say it is about, and `Sidecar` passes
  over a sheet declaring another track. Names are folded through core's `folded_letters` and agree
  where they fold to the same letters or one is a run of whole words of the other ("Echoes" answers
  for "Echoes (Live at Pompeii)", not "Hallelujah" by another artist); scripts no space divides
  (kana, CJK, Thai) agree by containment
  (`a_short_title_agrees_with_a_longer_one_only_by_whole_words`). Where one side folds to ASCII and
  the other does not they are different scripts and the check stands aside. A sheet declaring none of
  the three, or beside a row nothing has named, is read as always.
- **A declared length is a backstop, not a matcher.** `LENGTH_MAY_DIFFER_BY` is thirty seconds: a
  remaster moves a length a second or two, while two different songs sit within half a minute often
  enough that looser buys nothing. No declared length, or none published (`Wanted::duration`), reads
  as before.
- **A row cut out of a file gets its own share of the file's words, on its own clock.**
  `Wanted::cut_of_the_file` turns a whole-file set into the row's by `Lyrics::within`. An unsynced
  set goes to no row, and a cut with no `rate` is handed nothing rather than the album. The file's
  words are a sidecar named after the file (`NamedAfter::TheFile`) and whatever `Embedded` carries; a
  sidecar named after the row's own title is already the row's.
- **A `.lyricsfile.yaml` beside a file is a sidecar, the richest.** `BESIDE` pairs each ending with
  how it is `Written`, `.lyricsfile.yaml` and `.yml` ahead of `.lrc` and `.txt`. It is read to
  `LARGEST_LYRICSFILE` and refused past it rather than cut, YAML being no use in halves; its
  `metadata` feeds the same title, artist and duration check, so one declaring another song, or not
  parsing, gives way to the `.lrc` beside it
  (`a_lyricsfile_beside_the_track_is_read_word_by_word_ahead_of_an_lrc`).
- **The sidecar walks folders rather than trying fixed names** (`Place::Beside`, `Place::Within`;
  `WITHIN` is `lyrics`, `lyric`, `lrc`, matched case-insensitively). `Rank` is the whole precedence, in order: `.lrc` over `.txt`, then how the name was
  arrived at (file stem, whole name, stem and a language such as `Echoes.pt-BR.lrc`, then
  `<artist> - <title>` and `<title>` off `Wanted`), and only then where it was found. Tag spellings
  are compared for equality after `lrc::folded`, not containment, which would hand every sheet holding
  the title to every track. A candidate is read to `LARGEST_SIDECAR` (the reader's `LARGEST_SHEET`),
  keeping the whole lines that fitted. An unreadable candidate is logged and the walk goes on, so an
  oversized `Echoes.lrc` does not stop `Echoes.txt`.
- **A folder is walked once and remembered**: `Walked` holds at most `FOLDERS_WALKED` folders' file
  names, so an album costs one `read_dir` per folder. A walk is reused while the folder's modified
  time is unchanged, but only where that stamp is more than `TIMESTAMPS_SETTLE_IN` older than the
  walk, since a coarse inode clock stamps a folder written in the tick it was walked.

## The LRC reader

- **A line ends at `\n`, `\r\n` or a lone `\r`.** `lrc::lines_of` is the one splitter (`str::lines`
  reads an old Mac sheet as one line).
- **The ten LRC id tags are nine meanings, and the closed set a bracket must name to be a tag**
  (`Identified::NAMED`, `re` and `tool` being one). `ti`, `ar` and `length` are `Declared`; `al`,
  `au`, `by`, `re`/`tool` and `ve` are the `Credits`; `offset` shifts every moment. A bracket neither
  a moment nor one of the ten is the line it was written on (a `[Chorus]` marker prints): what cannot
  be read as structure is content, as in `Search::read`.
- **A value that is not arithmetic is as if unwritten.** A blank tag is passed over, an unparsable
  `[offset:]` leaves the shift where it was, and an unparsable `[length:]` leaves the sheet declaring
  none rather than zero, which would make every sheet name another track. `span` reads a length
  (`mm:ss`, `mm:ss.xx`, `h:mm:ss`, told apart by counting colons); a *moment* tries `moment` first so
  `[00:04:75]` keeps its hundredths, and `span` only where that fails.
- **A line is repeated once per timestamp written on it**, hence the reader's bounds: `LARGEST_SHEET`,
  `LARGEST_SET` and `MOST_LINES`, a sheet past any being `Error::Unreadable { op: Parse }`. They live
  in the reader because `Embedded` is handed a tag from an untrusted file.
- **A stamp inside a line times the word after it (enhanced LRC).** `Stamped::read` splits a line on
  every `<…>` that reads as a moment; what does not (`I <3 you`, `<b>`) stays text. Each stamp opens a
  word running to the next; text before the first stamp folds into the first word, a stamp with
  nothing after is the line's end. Words go through `[offset:]` with the line, and a line with several
  moments carries its words to each. Being in the one reader, every source yields word timing; the
  search index strips the stamps (`store::sung_words`), so `lyrics:` never reaches one.
- **A timed line may name one of two voices with `[v1: words]` or `[v2: words]` after its
  timestamp.** The marker is removed from the text; a line without one is voice one, and an
  unrecognised bracket stays text.
