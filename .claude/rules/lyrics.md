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
  moment and a `Voice` — one by default, two where a timed LRC line names it. Two voices can
  overlap; a line stays lit until it is judged sung, its own declared end, or at most ten seconds
  (below). A line may carry its words, timed one by one; a translation beside the original still
  has no representation. `Wanted` is the other half — the track a provider is asked about —
  carrying the title, artist, album and length a provider would search on and the file's own text
  as `Wanted::carried`.
- **`Lyrics::line_at` is what the pane reads at, `voices_in_play` what it lights, and they part
  company.** `line_at` is the last line whose moment has passed, what the pane centres on, so a long
  instrumental holds its place. The two voice slots stay lit up to each voice's next line or
  `LIT_AT_MOST` (10 s), so one singer entering does not put out the other. `line_in_play` is the
  latest active line; with neither voice active the set dims while the pane stays put.
- **A `Credits` is what a sheet claims about its own making, riding on the set.** It carries
  `[al:]`, `[au:]`, `[by:]`, `[re:]`/`[tool:]` and `[ve:]` as `album`, `words_by`, `sheet_by`,
  `editor` and `version`; `Lyrics::credited` is the one way one is attached — a builder rather than
  a fourth argument, so `Lyrics::synced` and `Lyrics::plain` stay the positional pair a provider
  ignorant of id tags calls — and `Lyrics::credits` is what the window reads. It is the sheet's word
  on *itself*, not on which track it is, which is why an embedded set carries credits though it is
  checked against nothing.
- **A line may carry its words, each with its own moment, and the end the sheet gave it.**
  `LyricLine::worded` takes `SungWord`s and makes the line's text their joining, so a word is a byte
  range of the line and no second copy can disagree; `LyricLine::ending` sets `until`, refused
  before the line starts, and `SungWord::ending` likewise for a word. `Lyrics::detail` is
  `Unsynced`, `Lines` or `Words` — the order `Lyricists::find` ranks by and the pane's badge.
  `LyricLine::sweep_at` is what the pane lights a word-timed line by: a `Sweep` of how many bytes
  are sung and the `Singing` word with how far through it the transport is, a word lasting to its
  own end, else the next word, else the line's end — one with none of the three counted sung once it
  starts. `Lyrics::within` shifts a line's end and words onto a cue row's clock with the line.
- **The pane sweeps a word-timed line word by word.** Only a line `voices_in_play` names is swept:
  unsung words at `UNSUNG_SHARE` of the lit colour, sung ones at all of it, and the word being sung
  *wiped* letter by letter — `wiped` lays how far through the word the transport is over its
  letters (trailing space left out, so the wipe ends on the last drawn), lights every letter behind
  the edge and mixes the one under it by the share passed, a multi-byte letter being one letter. It
  is all highlight runs over one `StyledText`, so the line wraps as it would unswept. While a word is
  sung the pane asks for a frame each frame, the position read afresh every 16 ms poll, so the sweep
  moves at the display's rate, not the transport rail's step. The heading's badge says
  `WORD-SYNCED` for such a set.
- **A line goes out once sung, and a blank line is a pause.** A synced set carries when a line
  starts, not when it ends, so `span_of` guesses: `sung_for` is `SUNG_BEFORE_THE_WORDS` plus
  `SUNG_PER_LETTER` a letter, held between `SUNG_AT_LEAST` and `LIT_AT_MOST`, and a line goes out
  there only where the next line of its voice is at least `A_BREATH_AT_LEAST` further on —
  otherwise it stays lit until that line (held to ten seconds), so a verse never flickers between
  lines. **A line whose sheet gave it an end goes out exactly there** — its own `until` or its last
  word's; the guess is for a line with neither. A timed blank line, how an `.lrc` marks the break
  between verses, ends the line before it and is never in play itself, so the pause counts down to
  the next written line. Before this a line held up to ten seconds whatever it said and a blank one
  counted as sung, so the dots were only seen before the first line. `has_ended` is the reading past
  the last written line.
- **`breathes_before` is `waiting_at` read ahead of time**: whether any position could ever wait on
  a line, by the same arithmetic — the first written line always, any other where the line before
  goes out `A_BREATH_AT_LEAST` or more ahead of it — so the pane gives that line its room once, at
  layout, not as the wait begins.
- **`waiting_at` is what a flat set can say about a gap.** Only where nothing is in play: which line
  the wait is for and how far through it the transport is, counting from the track's start before
  the first line and from the moment the last went out after it. It lives here, not in the window,
  as the one arithmetic that must agree with `LIT_AT_MOST` — a constant duplicated in `resonate-ui`
  would be a second thing to keep in step. How far through a *line* the transport is has no reader
  for a set without word timing: it could only be the whole span pretending to be a karaoke sweep.

## Providers

- **Lyrics come out of the file, and one LRC reader is the whole of how.** `Sidecar` reads an
  `.lrc` or `.txt` beside a local file or in a lyrics folder next to it; `Embedded` reads
  `Wanted::carried`, which is `TagSet::lyrics` — `USLT`, `LYRICS`, `UNSYNCEDLYRICS`,
  `UNSYNCED LYRICS` or `©lyr` — reaching the pane through the `StreamDigest` like every tag. ID3's
  `SYLT` reaches symphonia as a raw frame and `resonate-codec`'s `sylt.rs` writes it out as LRC — a
  line per entry, or, where entries open with a newline, the syllables between two newlines gathered
  into one line at the first one's moment, each syllable after an inline `<mm:ss.xxx>` stamp of its
  own, so a syllable-timed frame is a word-timed line — read by the same reader and outranking an
  unsynchronised frame beside it. A frame timed in MPEG frames rather than milliseconds is left
  unread, nothing saying how long a frame is. Both parse through the same `lrc` reader, so a tag with
  timestamps is a synced set exactly as a file would be, and a sidecar outranks the file's own as
  the deliberate one. `read_lyrics(source, text)` is the reader's one public door — `lrc::read` with
  its `Sheet` folded to its `Lyrics` — and `Embedded` and the online crate's `Lrclib` both use it, so
  tag text and service text are read under the same bounds and the `Sheet`'s `Declared` stays
  `Sidecar`'s alone.
- **A provider reaching a network lives in `resonate-online`, what it keeps in the library.**
  `Lrclib` is a `LyricProvider` like the two here, appended by the binary through `Lyricists::and`
  where `online` is on (`online.md` has how it asks). This crate carries `Wanted::span` for it: a
  cue row is a span of a file and the catalog's `lyrics_kept` is keyed by `(path, span_start)` as
  `tracks` is, so a `Wanted` carries the `Option<FrameSpan>` the window fills from the queue item,
  and a provider keeping nothing ignores it. The kept row is read before any request and a miss is
  remembered for a week (`sung.rs`), so a track the service has no words for costs one request a
  week. The Lyricsfile reader is this crate's — `read_lyricsfile`, over `serde-saphyr` — so LRCLIB's
  answer and a sidecar are read by one reader, and a build without `online` still reads the sidecar.
- **`Lyricists` walks the providers, and a refusal is not an answer.** `Lyricists::local` registers
  the two behind `Unsourced`, which answers nothing and is the one source `has_a_source` discounts,
  so a build with neither says no source is configured rather than that the track has no words. A
  refusing provider is logged and the walk continues; a refusal is reported only where nothing else
  answered. **The walk hands over the most finely timed set, not the first**: every provider is
  asked and an answer replaces the one held only where its `Detail` is higher, so order breaks a tie
  — a sidecar still outranks the tags where both are line-synced — and a file's plain words give way
  to a synced set fetched for it. A word-timed set ends the walk, nothing finer existing.
- **A sidecar is checked against its track; an embedded set never is.** `lrc::read` answers a
  `Sheet` — the `Lyrics`, credits and all, and a `Declared` of what its `[ti:]`, `[ar:]` and
  `[length:]` say it is about. `Sidecar` passes over a sheet declaring another track, folded to
  alphanumerics and case with either side allowed to contain the other, so "Echoes" answers for
  "Echoes (Live at Pompeii)" while Jeff Buckley's "Hallelujah" does not answer for Leonard Cohen's.
  A sheet declaring none of the three is read as always, as is one beside a row nothing has named.
  `Embedded` never checks: the text came out of the file.
- **A row cut out of a file gets its own share of the file's words, on its own clock.** `Wanted`
  carries the row's `span` and the stream's `rate`, and `Wanted::cut_of_the_file` turns a
  whole-file set into the row's: `Lyrics::within` keeps the synced lines inside the span and
  shifts them by its start, so the row's first line is timed from its own zero. An unsynced set
  cannot be cut and goes to no row; a set whose lines all fall outside is none. The file's is a
  sidecar named after the file (`Rank::named_after` answers `NamedAfter::TheFile` for the two names
  taken from the path) and whatever `Embedded` carries; a sidecar named after the row's own title is
  already the row's and handed over whole. A cut with no rate is handed nothing rather than the
  album.
- **A declared length is a backstop, not a matcher.** `LENGTH_MAY_DIFFER_BY` is thirty seconds: a
  sheet rounds to the second and a trim or remaster moves it a second or two, so tighter would throw
  away good sheets, and two different songs sit within half a minute often enough that looser buys
  nothing. It catches the single edit's sheet copied beside the twenty-three-minute album version,
  and can be that loose because the sidecar is already named after the file or tags — the last word
  against a sheet for another pressing, not how one is found. A sheet declaring no length, and a
  track whose length the engine has not published, are read as before; `Wanted::duration` is what
  the window fills for it. `Embedded` is exempt as from the title.
- **A `.lyricsfile.yaml` beside a file is a sidecar, the richest.** `BESIDE` pairs each ending the
  walk takes with how it is `Written` — `.lyricsfile.yaml` and `.lyricsfile.yml` ahead of `.lrc` and
  `.txt` — and an ending is stripped whole, so `Echoes.lyricsfile.yaml` is named after `echoes` like
  `Echoes.lrc`. It is read to `LARGEST_LYRICSFILE` and refused past it rather than cut at a line,
  YAML being no use in halves; its `metadata` gives the `title`, `artist` and `duration_ms` the
  `[ti:]`/`[ar:]`/`[length:]` check weighs, so one declaring another song — or not parsing — gives
  way to the `.lrc` beside it. `a_lyricsfile_beside_the_track_is_read_word_by_word_ahead_of_an_lrc`
  is the claim.
- **A name is weighed in the letters `folded_letters` spells, and a transliteration is no
  disagreement.** `lrc::folded` folds through `resonate_core::folded_letters` (the catalog's fold,
  moved into core for this), so `Przybylowicz` agrees with `Przybyłowicz` and `KISKANC` with
  `Kıskanç`; where one side folds to ASCII and the other does not, they are different scripts and
  nothing here can say whether *Kukla* is *Кукла*, so the check stands aside. Two names in one script
  are weighed as before.
- **The sidecar walks folders rather than trying fixed names, in two places** (`Place::Beside` and
  `Place::Within`). `WITHIN` is the folder names it descends into — `lyrics`, `lyric`, `lrc` —
  matched case-insensitively against entries the parent walk already listed, so `Lyrics/` in any
  casing is found without a stat per guess. `Rank` is the whole precedence, read in order: `.lrc`
  over `.txt`, then how the name was arrived at — the file's stem, its whole name, then
  `<artist> - <title>` and `<title>` off `Wanted` — and only then where it was found, so a sheet
  beside the track outranks the same name in `Lyrics/`, while one named exactly after the file
  outranks a tag-named one beside it. Tag spellings go through `lrc::folded` and are compared for
  equality, not containment: `pink_floyd echoes.lrc` answers for `01 Meddle side two.flac`, where
  containment would hand every sheet holding the title to every track. A track nothing has named
  offers no tag spellings. It reads at most `lrc::LARGEST_SHEET` bytes of a candidate and keeps the
  whole lines that fitted, rather than weighing `fs::metadata` and refusing: a sheet whose words are
  followed by an overlong tail is read as its words. The half line the bound cut is dropped, so a
  sheet whose first line outruns the reader holds nothing. The one bound is written once —
  `LARGEST_SIDECAR` *is* the reader's. An unreadable candidate is logged and the walk goes on (the
  rule `Lyricists` follows), so an oversized `Echoes.lrc` does not stop `Echoes.txt`, a refusal
  reported only where nothing else answered; an unreadable lyrics folder is logged and left, the
  sheet beside the track still to be reached.
- **A folder is walked once and remembered**, a track change having cost a directory read.
  `Walked` sits behind a `parking_lot::Mutex` on `Sidecar` (`LyricProvider::lyrics` takes `&self`)
  and holds at most `FOLDERS_WALKED` folders' file names as `Arc<[PathBuf]>`, the last looked at
  first, so an album costs one `read_dir` for the folder and one for its lyrics folder rather than
  two per track, and the cap stops a shuffle across a library remembering all of it. A walk is safe
  to reuse by the folder's modified time, taken in one stat: a sheet dropped in beside a playing
  track moves it. A timestamp alone is not enough — Linux stamps an inode from a coarse clock ticking
  once a jiffy and a filesystem may round to the second, so a folder written in the tick it was
  walked carries the walk's own time — so `TIMESTAMPS_SETTLE_IN`: a walk is read back only where the
  folder's stamp is more than a second older than the walk, so a folder still being written is
  walked again.

## The LRC reader

- **The ten LRC id tags are nine meanings, and the closed set a bracket must name to be a tag.**
  `Identified::NAMED` is the whole table, ten names against nine variants (`re` and `tool` being one
  meaning). Each lands somewhere: `ti`, `ar` and `length` are `Declared`, what the sheet is *about*
  and all `Sidecar` weighs; `al`, `au`, `by`, `re`/`tool` and `ve` are the set's `Credits`, what it
  says about *itself*; `offset` is the shift every moment is read through. Nothing is recognised and
  dropped. A bracket neither a moment nor one of the ten is the line it was written on, which prints
  a `[Chorus]` marker — and a writer's own tag under a name the spec never defined — rather than
  swallowing it; the rule `Search::read` follows for a token naming no field: what cannot be read as
  structure is content, so nothing fails to parse.
- **A value that is not arithmetic is as if unwritten.** A blank tag is passed over, an unparsable
  `[offset:]` leaves the shift where it was, and an unparsable `[length:]` leaves the sheet
  declaring none rather than zero (which would make every sheet name another track). `span` reads
  one: `mm:ss`, `mm:ss.xx` and `h:mm:ss`, told apart by counting colons rather than trying `moment`,
  which reads the second colon of `[00:04:75]` as a fraction separator and would take `1:03:20` for
  a minute and three seconds. A *moment* is the other way round: `moment` first, so `[00:04:75]`
  keeps its hundredths, and `span` only where that fails, so `[1:02:03.45]` (its fraction holding a
  dot) is sung an hour in rather than printed.
- **A line is repeated once per timestamp written on it**, which is why the reader bounds itself:
  `LARGEST_SHEET` is what it reads at all, `LARGEST_SET` and `MOST_LINES` what its lines may come
  to, and a sheet past any is `Error::Unreadable { op: Parse }`. The bound lives in the reader
  because `Embedded` is handed a tag from an untrusted file and `Sidecar` has cut its read to the
  same length already, so what reaches `LARGEST_SHEET` from a sidecar is only text a lossy decode
  expanded.
- **A stamp inside a line times the word after it — enhanced LRC.** `Stamped::read` splits a line
  on every `<…>` that reads as a moment; what does not (`I <3 you`, `<b>`) stays text. Each stamp
  opens a word running to the next; text before the first stamp folds into the first word, a stamp
  with nothing after is the line's end, and a space on both sides of a stamp is kept once, so the
  words join to the line as it reads. A line of stamps with no `[mm:ss]` is sung from its first
  word. Words go through `[offset:]` with the line, and a stamped line given several moments carries
  its words to each by the distance from the first. In the one reader, so a sidecar, a tag, LRCLIB's
  synced text and the catalog's kept text all yield word timing; the search index strips the stamps
  (`store::sung_words`), so `lyrics:` never reaches one.
- **A timed line may name one of two voices with `[v1: words]` or `[v2: words]` after its
  timestamp.** The marker is removed from the text and stays with each repeated timestamp and a cue
  row's shifted line; a line without one is voice one, and an unrecognised bracket stays text.
  Sidecars, embedded tags and fetched sets alike, through the one reader.
