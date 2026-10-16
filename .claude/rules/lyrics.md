---
paths:
  - "crates/resonate-lyrics/**/*.rs"
  - "crates/resonate-ui/src/lyrics.rs"
  - "crates/resonate-ui/src/views/lyrics.rs"
  - "crates/resonate-codec/src/sylt.rs"
  - "crates/resonate-online/src/lrclib.rs"
  - "crates/resonate-library/src/sung.rs"
  - "crates/resonate-mobile/src/lyrics.rs"
---

# Lyrics

`resonate-lyrics`: vocabulary and provider seam; `resonate-ui` draws it. No gpui, engine or library
dependency (tested without a window).

## The vocabulary

- **`Lyrics`**: flat `LyricLine`s, `Synced` or `Unsynced`. A line has an optional moment and a
  `Voice` (one by default; two where a timed LRC line names it). `Wanted` = the track a provider is
  asked about; its `carried` field is the file's own text.
- **`line_at` (pane centres on it) and `voices_in_play` (pane lights it) part company.** `line_at` =
  last line whose moment has passed (a long instrumental holds its place). Each voice stays lit to
  its next line or `LIT_AT_MOST`, so one singer entering does not put out the other; with neither
  active the set dims and the pane stays put. `Lyrics` notes at construction whether any line names
  the second voice, so a one-voice sheet reads its latest passed line alone, never the lines before.
- **`Credits`**: what a sheet claims about its own making, not which track it is; embedded sets
  carry them unchecked.
- **A line may carry words, each with a moment, and the sheet's end for it.** `LyricLine::worded`
  makes the text the words' joining (a word is a byte range of the line: no second copy to
  disagree). `Lyrics::detail` (`Unsynced`, `Lines`, `Words`) drives `Lyricists::find`'s ranking and
  the pane's badge. `LyricLine::sweep_at` gives `Sweep` (bytes sung) and the `Singing` word.
  `Lyrics::within` shifts a line onto a cue row's clock.
- **`Sweep::wiped` is the one wipe**, the desktop's highlight and the phones' `LyricWipe` alike:
  the singing word's letters (trailing space left out) are wiped `through × letters` across, whole
  letters sung, the next `Blending` at the fraction left; with no word singing the line is sung to
  `Sweep::sung`. `Wiped` names byte offsets; whoever indexes otherwise converts them
  (`a_word_being_sung_is_wiped_across_letter_by_letter`).
- **The pane sweeps a word-timed line** (only one `voices_in_play` names): unsung words sit at
  `UNSUNG_SHARE` of the way to the lit colour, the sung word is wiped letter by letter; the
  highlight runs over one `StyledText` so the line wraps as unswept. **A mark never wraps alone**:
  gpui breaks before any non-word character, so a closing quote or `!` could stand on a line of its
  own; `kept_with_its_marks` asks gpui's `LineWrapper` where it would break and turns the space before
  that word into a newline (same byte length, so a sweep's offsets hold; a run of letters such as CJK
  is left to gpui).
- **A line goes out once sung; a blank line is a pause.** Synced sets carry starts, not ends, so
  `span_of` guesses: `sung_for` = `SUNG_BEFORE_THE_WORDS` + `SUNG_PER_LETTER` per letter, clamped to
  `SUNG_AT_LEAST`..`LIT_AT_MOST`; the line goes out there only where its voice's next line is at
  least `A_BREATH_AT_LEAST` further on, else it stays lit to that line (no verse flicker). **Across
  two voices the same holds for whichever line comes next** (`answered`), else the pane stands empty
  between singers (`a_line_waits_for_the_other_voice_rather_than_leaving_less_than_a_breath_unlit`).
  **A sheet-given end goes out exactly there.** A timed blank line ends the line before it and is
  never in play; the pause counts down to the next written line. `has_ended`: past the last line.
- **`waiting_at` says what a flat set says about a gap; `breathes_before` reads the same ahead.**
  Only where nothing is in play; counted from the later of each voice's last line to go out
  (`a_wait_counts_from_whichever_voice_went_out_last`). They live here as the one arithmetic that
  must agree with `LIT_AT_MOST`.

## Providers

- **Lyrics come out of the file; one LRC reader is the whole of how.** `Sidecar`: `.lrc`/`.txt`
  beside a local file or in a lyrics folder next to it. `Embedded`: `Wanted::carried` =
  `TagSet::lyrics` (`USLT`, `LYRICS`, `UNSYNCEDLYRICS`, `UNSYNCED LYRICS`, `©lyr`). ID3 `SYLT`
  arrives as a raw frame; `resonate-codec`'s `sylt.rs` writes it as LRC (syllable-timed entries
  become a word-timed line), outranking an unsynchronised frame beside it. A frame timed in MPEG
  frames is turned to milliseconds by `mpa::FrameClock` (samples a frame from the codec and rate:
  MP1 384, MP2 and MPEG-1 MP3 1 152, MPEG-2/2.5 MP3 576; priming not weighed); it is unread on any
  other codec, or whose content type is not lyrics/transcription. A UTF-16 text with no byte-order
  mark is big-endian, as ID3 says; a mark seen sets the order for the texts after it.
  `MOST_SYLLABLES` bounds the read;
  of several language frames the first stands. The sidecar wins ties over the file's own (the
  deliberate one). `read_lyrics(source, text)` is the reader's one public door (`Embedded`,
  `Lrclib`), so tag and service text share the bounds.
- **Network providers live in `resonate-online`, keeping results in the library.** `Lrclib` is a
  `LyricProvider` the binary appends via `Lyricists::and` where `online` is on (`online.md`). A cue
  row is a span: `Wanted::span` keys the catalog's `lyrics_kept` by `(path, span_start)`, as `tracks`
  is. A kept row is read before any request; a miss is remembered (`MISSED_AGAIN_AFTER`, `sung.rs`).
  `read_lyricsfile` is this crate's, so a build without `online` still reads the sidecar.
- **`Lyricists` walks the providers; a refusal is not an answer.** `Lyricists::local` registers the
  two behind `Unsourced`, the one source `has_a_source` discounts (neither: "no source configured",
  not "track has no words"). A refusing provider is logged and the walk goes on; a refusal is
  reported only where nothing else answered. **The walk hands over the most finely timed set, not
  the first**: an answer replaces the held one only on a higher `Detail` (order breaks ties); a
  word-timed set ends the walk. **Pane lookup is `LyricsModel::_find`, one task**: a row leaving the
  queue (no `Wanted`) drops it, so a running lookup cannot land the last track's words over none.
- **A sidecar is checked against its track; an embedded set never is.** `lrc::read` answers a
  `Sheet` with a `Declared` of what `[ti:]`, `[ar:]`, `[length:]` say; `Sidecar` passes over a sheet
  declaring another track. Names fold through core's `folded_letters` and agree where they fold to
  the same letters or one is a run of whole words of the other ("Echoes" answers for "Echoes (Live
  at Pompeii)", not "Hallelujah"); unspaced scripts (kana, CJK, Thai) agree by containment. One side
  ASCII-folding and the other not = different scripts: the check stands aside. A sheet declaring
  none of the three, or beside a row nothing has named, is read as always
  (`a_short_title_agrees_with_a_longer_one_only_by_whole_words`).
- **A declared length is a backstop, not a matcher.** `LENGTH_MAY_DIFFER_BY` = 30 s: a remaster moves
  a length a second or two, different songs often sit within half a minute. No declared length, or
  none published (`Wanted::duration`): read as before.
- **A cut row gets its share of the file's words, on its own clock.** `Wanted::cut_of_the_file` turns
  a whole-file set into the row's via `Lyrics::within`. An unsynced set gives no row; a cut with no
  `rate` gives nothing, not the album. The file's words = the sidecar named after the file
  (`NamedAfter::TheFile`) + whatever `Embedded` carries; a sidecar named after the row's own title
  is already the row's. **A whole-file sheet is checked against the whole file, then cut**
  (`Wanted::of_the_whole_file`: the album stands for the title; the row's artist and length are
  not the file's, so none is weighed), never against the row, whose title a `[ti:]` of the album
  would disagree with.
- **A `.lyricsfile.yaml` beside a file is a sidecar, the richest.** `BESIDE` pairs each ending with
  how it is `Written`: `.lyricsfile.yaml`, `.yml`, then `.lrc`, `.txt`. Read to `LARGEST_LYRICSFILE`,
  refused past it, not cut (YAML is no use in halves). `metadata` feeds the same
  title/artist/duration check; one declaring another song, or not parsing, gives way to the `.lrc`
  beside it (`a_lyricsfile_beside_the_track_is_read_word_by_word_ahead_of_an_lrc`).
- **The sidecar walks folders, not fixed names** (`Place::Beside`, `Place::Within`; `WITHIN` =
  `lyrics`, `lyric`, `lrc`, case-insensitive). `Rank` is the whole precedence, in order: `.lrc` over
  `.txt`; how the name was arrived at (file stem, whole name, stem + language like
  `Echoes.pt-BR.lrc`, then `<artist> - <title>` and `<title>` off `Wanted`); for a language sheet,
  how early its language stands among those the listener reads; only then where found. Tag
  spellings compare for equality after `lrc::folded`, not containment (else every sheet holding the
  title goes to every track). A candidate is read to `LARGEST_SIDECAR` (= the reader's
  `LARGEST_SHEET`), keeping whole lines: bytes are cut back to the last line break *before* their
  encoding is weighed, so a UTF-8 file cut inside a letter is not guessed a legacy code page
  (`a_sidecar_in_utf8_cut_inside_a_letter_still_reads_as_utf8`). An unreadable candidate is logged
  and the walk goes on (an oversized `Echoes.lrc` does not stop `Echoes.txt`).
- **A `.txt` is words only where it reads as words** (`Written::Text`, `reads_as_sung`): an unsynced
  set from one is passed over unless fewer than one line in `UNSUNG_LINES_AT_MOST_ONE_IN` is
  unsung: past `LONGEST_SUNG_LINE`, under half letters, holding a link, or a `name: value`/`name =
  value` field of at most `LONGEST_FIELD_NAME_IN_WORDS` words. A rip log or liner note named like
  the track is no lyric (`a_text_file_that_reads_as_a_document_is_not_taken_for_words`); a `.txt`
  carrying stamps, and every `.lrc`, is read as always.
- **Languages are the locale's, only where asked.** `Sidecar::choosing_by_the_locale` reads
  `LANGUAGE`'s colon list (unless the locale is `C`/`POSIX`), then the first of `LC_ALL`,
  `LC_MESSAGES`, `LANG`, each through `language_of`; weighed only while its `Arc<AtomicBool>` holds
  (`lyrics-language-from-locale`, off by default; `Sidecar::default` and `Lyricists::local` hold one
  that never does). A Japanese reader gets `Song.ja.lrc` beside `Song.en.lrc`; else, and for a
  reader of neither, first in name order
  (`of_two_sheets_in_a_language_the_one_the_listener_reads_answers`).
- **A folder is walked once and remembered**: `Walked` holds at most `FOLDERS_WALKED` folders' file
  names. Reused while the folder's modified time is unchanged, but only if that stamp is more than
  `TIMESTAMPS_SETTLE_IN` older than the walk (a coarse inode clock stamps a folder written in the
  tick it was walked).

## The LRC reader

- **Line ends: `\n`, `\r\n`, lone `\r`**: `lrc::lines_of` is the one splitter (`str::lines` reads an
  old Mac sheet as one line).
- **Ten LRC id tags, nine meanings: the closed set a bracket must name to be a tag**
  (`Identified::NAMED`; `re`, `tool` are one). `ti`, `ar`, `length`: `Declared`. `al`, `au`, `by`,
  `re`/`tool`, `ve`: `Credits`. `offset` shifts every moment. A bracket that is neither a moment nor
  one of the ten is the line it was written on (`[Chorus]` prints): what cannot be read as structure
  is content. Such a bracket ahead of a line's stamps heads that timed line
  (`a_marker_ahead_of_the_stamps_still_lets_them_time_the_line`).
- **A non-arithmetic value is as if unwritten.** Blank tag: passed over. Unparsable `[offset:]`:
  shift stays. Unparsable `[length:]`: the sheet declares none, not zero (zero would make every
  sheet name another track). `span` reads a length (`mm:ss`, `mm:ss.xx`, `h:mm:ss`, told apart by
  counting colons); a *moment* tries `moment` first so `[00:04:75]` keeps its hundredths.
- **A line repeats once per timestamp on it**, hence `LARGEST_SHEET`, `LARGEST_SET`, `MOST_LINES`;
  past any: `Error::Unreadable { op: Parse }`. In the reader because `Embedded` is handed a tag from
  an untrusted file. `model::MOST_LINES` is the one count: a Lyricsfile's `lines` past it are
  refused, and `Lyrics::plain` keeps no more than it of any plain text.
- **A stamp inside a line times the word after it (enhanced LRC).** `Stamped::read` splits a line on
  every `<…>` that reads as a moment; others (`I <3 you`, `<b>`) stay text. A `>` is looked for only
  within `LONGEST_WORD_STAMP` bytes of its `<`, so a line of unclosed brackets is one pass
  (`a_line_of_nothing_but_opened_stamps_is_read_in_one_pass`). Each stamp opens a word running to
  the next; text before the first stamp folds into the first word; a stamp with nothing after is the
  line's end. Words go through `[offset:]` with the line; a multi-moment line carries its words to
  each moment. The search index strips the stamps (`store::sung_words`), so `lyrics:` never reaches
  one.
- **Voices: `[v1: words]` / `[v2: words]` after a timed line's timestamp.** The marker is removed
  from the text; none = voice one; an unrecognised bracket stays text.
