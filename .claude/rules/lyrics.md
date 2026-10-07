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

`resonate-lyrics`: vocabulary and provider seam; `resonate-ui` draws it. No gpui, engine or library
dependency (tested without a window).

## The vocabulary

- **`Lyrics`**: flat `LyricLine`s, `Synced` or `Unsynced`. Line: optional moment, `Voice` (one by
  default; two where a timed LRC line names it). `Wanted` = track a provider is asked about; the
  file's own text is `Wanted::carried`.
- **`line_at` (pane centres on it) and `voices_in_play` (pane lights it) part company.** `line_at`
  = last line whose moment has passed (long instrumental holds its place). Each voice stays lit to
  its next line or `LIT_AT_MOST` (one singer entering must not put out the other); neither active:
  set dims, pane stays put.
- **`Credits`**: what a sheet claims about its own making, not which track it is; embedded sets
  carry them though checked against nothing.
- **A line may carry words, each with a moment, and the sheet's end for it.** `LyricLine::worded`
  makes the text the words' joining (word = byte range of the line: no second copy to disagree).
  `Lyrics::detail` (`Unsynced`, `Lines`, `Words`): `Lyricists::find`'s ranking and the pane's badge.
  `LyricLine::sweep_at`: `Sweep` (bytes sung) + `Singing` word with how far through the transport
  is. `Lyrics::within` shifts a line onto a cue row's clock.
- **Pane sweeps a word-timed line** (only one `voices_in_play` names): unsung words at
  `UNSUNG_SHARE` of the way to the lit colour, sung word *wiped* letter by letter; highlight runs
  over one `StyledText` so the line wraps as unswept.
- **A line goes out once sung; a blank line is a pause.** Synced sets carry starts, not ends, so
  `span_of` guesses: `sung_for` = `SUNG_BEFORE_THE_WORDS` + `SUNG_PER_LETTER`/letter, clamped
  `SUNG_AT_LEAST`..`LIT_AT_MOST`; goes out there only where its voice's next line is at least
  `A_BREATH_AT_LEAST` further on, else lit to that line (no verse flicker). **Across two voices the
  same holds for whichever line comes next** (`answered`), else the pane stands empty, no dots
  between singers
  (`a_line_waits_for_the_other_voice_rather_than_leaving_less_than_a_breath_unlit`). **A sheet-given
  end goes out exactly there.** A timed blank line ends the line before it, is never in play; the
  pause counts down to the next written line. `has_ended`: reading past the last line.
- **`waiting_at`: what a flat set says about a gap; `breathes_before`: same read ahead.** Only
  where nothing is in play: which line the wait is for, how far through, counted from the later of
  each voice's last line to go out (`a_wait_counts_from_whichever_voice_went_out_last`). Both live
  here as the one arithmetic that must agree with `LIT_AT_MOST`; the pane gives a line its room
  once, at layout.

## Providers

- **Lyrics come out of the file; one LRC reader is the whole of how.** `Sidecar`: `.lrc`/`.txt`
  beside a local file or in a lyrics folder next to it. `Embedded`: `Wanted::carried` =
  `TagSet::lyrics` (`USLT`, `LYRICS`, `UNSYNCEDLYRICS`, `UNSYNCED LYRICS`, `©lyr`). ID3 `SYLT`
  arrives as a raw frame; `resonate-codec`'s `sylt.rs` writes it as LRC (syllable-timed entries
  become a word-timed line), outranking an unsynchronised frame beside it. Unread: frame timed in
  MPEG frames, or content type not lyrics/transcription. `MOST_SYLLABLES` bounds the read; of
  several language frames the first stands. Sidecar wins ties over the file's own (the deliberate
  one). `read_lyrics(source, text)` = the reader's one public door (`Embedded`, `Lrclib`): tag and
  service text share the bounds.
- **Network providers live in `resonate-online`, keeping results in the library.** `Lrclib`: a
  `LyricProvider` the binary appends via `Lyricists::and` where `online` is on (`online.md`). A cue
  row is a span: `Wanted::span` keys the catalog's `lyrics_kept` by `(path, span_start)`, as
  `tracks` is. Kept row read before any request; miss remembered (`MISSED_AGAIN_AFTER`, `sung.rs`).
  `read_lyricsfile` is this crate's (a build without `online` still reads the sidecar).
- **`Lyricists` walks the providers; a refusal is not an answer.** `Lyricists::local` registers the
  two behind `Unsourced`, the one source `has_a_source` discounts (neither: "no source configured",
  not "track has no words"). A refusing provider is logged, walk goes on; refusal reported only
  where nothing else answered. **Walk hands over the most finely timed set, not the first**: an
  answer replaces the held one only on a higher `Detail` (order breaks ties); a word-timed set ends
  the walk. **Pane lookup = `LyricsModel::_find`, one task**: a row leaving the queue (no `Wanted`)
  drops it, so a running lookup cannot land the last track's words over *none*.
- **A sidecar is checked against its track; an embedded set never is.** `lrc::read` answers a
  `Sheet` with a `Declared` of what `[ti:]`, `[ar:]`, `[length:]` say; `Sidecar` passes over a sheet
  declaring another track. Names fold through core's `folded_letters`; agree where they fold to the
  same letters or one is a run of whole words of the other ("Echoes" answers for "Echoes (Live at
  Pompeii)", not "Hallelujah" by another artist); unspaced scripts (kana, CJK, Thai) agree by
  containment. One side ASCII-folding, other not = different scripts: check stands aside. A sheet
  declaring none of the three, or beside a row nothing has named: read as always
  (`a_short_title_agrees_with_a_longer_one_only_by_whole_words`).
- **A declared length is a backstop, not a matcher.** `LENGTH_MAY_DIFFER_BY` = 30 s: a remaster
  moves a length a second or two; different songs often sit within half a minute (looser buys
  nothing). No declared length, or none published (`Wanted::duration`): read as before.
- **A cut row gets its share of the file's words, on its own clock.** `Wanted::cut_of_the_file`
  turns a whole-file set into the row's via `Lyrics::within`. Unsynced set: no row. Cut with no
  `rate`: nothing, not the album. File's words = sidecar named after the file
  (`NamedAfter::TheFile`) + whatever `Embedded` carries; a sidecar named after the row's own title
  is already the row's.
- **A `.lyricsfile.yaml` beside a file is a sidecar, the richest.** `BESIDE` pairs each ending with
  how it is `Written`: `.lyricsfile.yaml`, `.yml`, then `.lrc`, `.txt`. Read to
  `LARGEST_LYRICSFILE`, refused past it, not cut (YAML is no use in halves). `metadata` feeds the
  same title/artist/duration check; one declaring another song, or not parsing, gives way to the
  `.lrc` beside it (`a_lyricsfile_beside_the_track_is_read_word_by_word_ahead_of_an_lrc`).
- **The sidecar walks folders, not fixed names** (`Place::Beside`, `Place::Within`; `WITHIN` =
  `lyrics`, `lyric`, `lrc`, case-insensitive). `Rank` is the whole precedence, in order: `.lrc` over
  `.txt`; how the name was arrived at (file stem, whole name, stem + language like
  `Echoes.pt-BR.lrc`, then `<artist> - <title>` and `<title>` off `Wanted`); for a language sheet,
  how early its language stands among those the listener reads; only then where found. Tag
  spellings compare for equality after `lrc::folded`, not containment (else every sheet holding the
  title goes to every track). A candidate is read to `LARGEST_SIDECAR` (= the reader's
  `LARGEST_SHEET`), keeping the whole lines that fitted: the bytes are cut back to the last line
  break *before* their encoding is weighed (an even length under a UTF-16 mark), so a UTF-8 file
  cut inside a letter is not guessed a legacy code page
  (`a_sidecar_in_utf8_cut_inside_a_letter_still_reads_as_utf8`). An unreadable candidate is logged, walk
  goes on (an oversized `Echoes.lrc` does not stop `Echoes.txt`).
- **Languages are the locale's, only where asked.** `Sidecar::choosing_by_the_locale` reads
  `LANGUAGE`'s colon list (unless locale is `C`/`POSIX`), then the first of `LC_ALL`,
  `LC_MESSAGES`, `LANG`, each through `language_of`; weighed only while its `Arc<AtomicBool>` holds
  (`lyrics-language-from-locale`, off by default; `Sidecar::default` and `Lyricists::local` hold one
  that never does). Asked: a Japanese reader gets `Song.ja.lrc` beside `Song.en.lrc`; else, and for
  a reader of neither, first in name order
  (`of_two_sheets_in_a_language_the_one_the_listener_reads_answers`).
- **A folder is walked once and remembered**: `Walked` holds at most `FOLDERS_WALKED` folders' file
  names (one `read_dir` per album folder). Reused while the folder's modified time is unchanged,
  but only if that stamp is more than `TIMESTAMPS_SETTLE_IN` older than the walk (a coarse inode
  clock stamps a folder written in the tick it was walked).

## The LRC reader

- **Line ends: `\n`, `\r\n`, lone `\r`**: `lrc::lines_of` is the one splitter (`str::lines` reads an
  old Mac sheet as one line).
- **Ten LRC id tags, nine meanings: the closed set a bracket must name to be a tag**
  (`Identified::NAMED`; `re`, `tool` are one). `ti`, `ar`, `length`: `Declared`. `al`, `au`, `by`,
  `re`/`tool`, `ve`: `Credits`. `offset` shifts every moment. A bracket neither a moment nor one of
  the ten is the line it was written on (`[Chorus]` prints): what cannot be read as structure is
  content, as in `Search::read`.
- **A non-arithmetic value is as if unwritten.** Blank tag: passed over. Unparsable `[offset:]`:
  shift stays. Unparsable `[length:]`: sheet declares none, not zero (zero would make every sheet
  name another track). `span` reads a length (`mm:ss`, `mm:ss.xx`, `h:mm:ss`, told apart by
  counting colons); a *moment* tries `moment` first so `[00:04:75]` keeps its hundredths, `span`
  only where that fails.
- **A line repeats once per timestamp on it**, hence `LARGEST_SHEET`, `LARGEST_SET`, `MOST_LINES`;
  past any: `Error::Unreadable { op: Parse }`. In the reader because `Embedded` is handed a tag from
  an untrusted file. `model::MOST_LINES` is the one count: a Lyricsfile's `lines` past it are
  refused, and `Lyrics::plain` keeps no more than it of any plain text (LRCLIB's, a Lyricsfile's
  `plain`).
- **A stamp inside a line times the word after it (enhanced LRC).** `Stamped::read` splits a line on
  every `<…>` that reads as a moment; others (`I <3 you`, `<b>`) stay text. A `>` is looked for
  only within `LONGEST_WORD_STAMP` bytes of its `<`, so a line of unclosed brackets is one pass
  (`a_line_of_nothing_but_opened_stamps_is_read_in_one_pass`). Each stamp opens a word
  running to the next; text before the first stamp folds into the first word; a stamp with nothing
  after is the line's end. Words go through `[offset:]` with the line; a multi-moment line carries
  its words to each moment. In the one reader, so every source yields word timing; the search index
  strips the stamps (`store::sung_words`), so `lyrics:` never reaches one.
- **Voices: `[v1: words]` / `[v2: words]` after a timed line's timestamp.** Marker removed from the
  text; none = voice one; an unrecognised bracket stays text.
