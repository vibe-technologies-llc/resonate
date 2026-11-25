---
paths:
  - "crates/resonate-lyrics/**/*.rs"
  - "crates/resonate-ui/src/lyrics.rs"
  - "crates/resonate-ui/src/views/lyrics.rs"
  - "crates/resonate-codec/src/sylt.rs"
  - "crates/resonate-online/src/lrclib.rs"
  - "crates/resonate-online/src/betterlyrics.rs"
  - "crates/resonate-online/src/lyrics.rs"
  - "crates/resonate-library/src/sung.rs"
  - "crates/resonate-mobile/src/lyrics.rs"
  - "crates/resonate/src/tagging.rs"
  - "crates/resonate-ui/src/models/saving_lyrics.rs"
  - "crates/resonate-mobile/src/saving_lyrics.rs"
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
- **A synced set may hold untimed lines, kept where they were written.** `LyricLine::untimed` in a
  `Lyrics::synced` set is an aside (a section head, a credit, a translation): it stands after the
  timed line written before it, whatever moment that line is sung at (`placed` groups each timed line
  with the asides after it, then sorts the groups; ones ahead of every timed line head the set), and
  `Lyrics::stands` holds the moment each line stands at (an aside's is its timed line's) so
  `passed` stays a partition point. An aside is never lit, read at or waited on
  (`LyricLine::can_be_lit`): `line_at`, `voices_in_play`, `waiting_at`, `went_out_before`,
  `has_ended` and `is_empty` skip it, and `span_of` ignores it when finding where its neighbour's
  voice next sings, so it never ends the timed line beside it early. `within` carries an aside with
  its timed line. `synced` refuses a set with no timed line (`Error::NoLineTimed`). The pane and the
  phones draw an aside as any unlit row, with no moment (`at_ms` none, so it is not a seek target),
  dimmed by the falloff like its neighbours.
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

## The clock

- **A line is lit at the moment heard, not the moment rendered, and no pane corrects for latency.**
  Every lyric clock reads `PlayerState`'s position, which the engine already publishes as the frame
  heard (`audio.md`, *What the engine publishes*): the desktop pane through `Heard::of` and
  `LyricsModel::keep_time`, the phones through the snapshot's `position_ms` handed to `lyric_frame`
  and `kept_lyric_frame`. So the ring, the chain's latency (a resampler at Very high, a convolver)
  and what the device or renderer reports it holds (a Bluetooth sink's included) are taken off once,
  in the engine, for the seek bar, MPRIS and the words alike
  (`a_lyric_line_lights_when_its_words_are_heard_past_the_chain_and_the_device_not_when_rendered`).
  A sheet's `[offset:]` and the per-song `LyricsAhead` (the desktop's pane, the phones'
  `nudge_lyrics`, `mobile.md`) move the words against that clock,
  for a sheet timed early or late, never for the output.

## Providers

- **Lyrics come out of the file; one LRC reader is the whole of how.** `Sidecar`: `.lrc`/`.txt`
  beside a local file or in a lyrics folder next to it. `Embedded`: `Wanted::carried` =
  `TagSet::lyrics` (`USLT`, `LYRICS`, `UNSYNCEDLYRICS`, `UNSYNCED LYRICS`, `©lyr`). ID3 `SYLT`
  arrives as a raw frame; `resonate-codec`'s `sylt.rs` writes it as LRC (syllable-timed entries
  become a word-timed line), outranking an unsynchronised frame beside it. A frame timed in MPEG
  frames is turned to milliseconds by `mpa::FrameClock` (samples a frame from the codec and rate:
  MP1 384, MP2 and MPEG-1 MP3 1 152, MPEG-2/2.5 MP3 576; counted from the first sample heard, the
  priming delay taken off, `FrameClock::heard_from`); it is unread on any
  other codec, or whose content type is not lyrics/transcription. A UTF-16 text with no byte-order
  mark is big-endian, as ID3 says; a mark seen sets the order for the texts after it.
  `MOST_SYLLABLES` bounds the read;
  of several language frames the first stands. Written back, a sheet that frame reads as is framed
  again as one (`sylt::timed`, `library.md`'s tag writing), so the read and the write agree. The
  sidecar wins ties over the file's own (the deliberate one). `read_lyrics(source, text)` is the reader's one public door (`Embedded`,
  `OnlineLyrics`), so tag and service text share the bounds.
- **Network providers live in `resonate-online`, keeping results in the library.** `OnlineLyrics`
  (LRCLIB and Better Lyrics, scored per song) is a `LyricProvider` the binary appends via
  `Lyricists::and` where `online` is on (`online.md`). A cue
  row is a span: `Wanted::span` keys the catalog's `lyrics_kept` by `(path, span_start)`, as `tracks`
  is. A kept row is read before any request; a miss is remembered (`MISSED_AGAIN_AFTER`, `sung.rs`).
  `read_lyricsfile` is this crate's, so a build without `online` still reads the sidecar.
  **A kept document is a Lyricsfile or a TTML**: `read_document` reads one starting with `<` as
  TTML, else as a Lyricsfile, so the catalog's one document column holds either.
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
  not the file's, so `Wanted::file_artist` (the cue's album artist) and `Wanted::file_duration`
  (`MediaInfo::whole_file`, the file's music length a confined decoder remembers) stand for them,
  each weighed only where known), never against the row, whose title a `[ti:]` of the album
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
  and the walk goes on (an oversized `Echoes.lrc` does not stop `Echoes.txt`). **A candidate is opened
  only if it is a regular file** (`regular.rs`: opened without blocking and without a controlling terminal on Unix, the kind
  read from the open handle), so a FIFO named `Song.lrc` is passed over at once rather than blocking
  the lookup thread; the write's read-back of a standing sheet does the same
  (`a_fifo_named_as_a_sidecar_is_passed_over_at_once_for_the_text_file_behind_it`). Names are
  compared lossily, so a track or sheet whose name is not UTF-8 still names its sheet.
- **A `.txt` is words only where it reads as words** (`Written::Text`, `reads_as_sung`): an unsynced
  set from one is passed over unless fewer than one line in `UNSUNG_LINES_AT_MOST_ONE_IN` is
  unsung: past `LONGEST_SUNG_LINE`, under half letters, holding a link, or a `name: value`/`name =
  value` field of at most `LONGEST_FIELD_NAME_IN_WORDS` words. A rip log or liner note named like
  the track is no lyric (`a_text_file_that_reads_as_a_document_is_not_taken_for_words`); a `.txt`
  carrying stamps, and every `.lrc`, is read as always.
- **Languages are the locale's, only where asked.** `Sidecar::choosing_by_the_locale` reads
  `resonate_words::languages_the_listener_reads` (`language.md`: `LANGUAGE`'s list unless the locale
  is `C`/`POSIX`, then the first of `LC_ALL`, `LC_MESSAGES`, `LANG`; on Windows the system's), each
  through `language_of`; weighed only while its `Arc<AtomicBool>` holds
  (`lyrics-language-from-locale`, off by default; `Sidecar::default` and `Lyricists::local` hold one
  that never does). A Japanese reader gets `Song.ja.lrc` beside `Song.en.lrc`; else, and for a
  reader of neither, first in name order
  (`of_two_sheets_in_a_language_the_one_the_listener_reads_answers`).
- **A folder is walked once and remembered**: `Walked` holds at most `FOLDERS_WALKED` folders' file
  names. Reused while the folder's modified time is unchanged, but only if that stamp is more than
  `TIMESTAMPS_SETTLE_IN` older than the walk (a coarse inode clock stamps a folder written in the
  tick it was walked).

## Writing a fetched sheet beside the track

- **Fetched words leave the catalog only when the listener asks, as a sidecar the reader takes
  back first** (`beside.rs`: `plan_beside`, `write_beside`). The form is what the catalog keeps:
  a Lyricsfile (`LyricText::lyricsfile` that is not TTML) as `<stem>.lyricsfile.yaml`, the richest
  ending `BESIDE` ranks; anything else, a TTML's lines included (no sidecar reads TTML), as the kept
  LRC text in `<stem>.lrc`, since every `.lrc` is read as always where a plain `.txt` might be
  taken for a document. So `Sidecar` reads back what the catalog holds
  (`a_synced_sheet_kept_is_written_beside_the_track_and_read_back_as_it_was_kept`,
  `a_lyricsfile_kept_is_written_as_the_richest_sidecar_and_read_back_word_by_word`). No audio file
  is touched.
- **A sheet already there is the listener's.** Any sheet named after the file beside it or in a
  lyric folder (`WITHIN`), whatever its case or ending, refuses the write (`Error::SidecarStands`)
  unless `Standing::Replaced` is asked; the one at the same name and ending reading as the very text
  (through `text::decoded`) is `Beside::AlreadySaid`. Even when replacing, a richer ending standing
  beside (a `.lyricsfile.yaml` ahead of the `.lrc` to be written) is never hidden behind a plainer
  one: `Error::SidecarOutranked`
  (`a_sheet_standing_beside_the_track_is_not_overwritten_unless_the_listener_replaces_it`,
  `a_richer_sheet_beside_the_track_is_never_hidden_behind_a_plainer_one`). A name past
  `NAME_BYTES_AT_MOST` is refused (`SidecarNameTooLong`): a cut name would never be read back.
- **Where the sheet goes is decided once, over names** (`SheetsBeside`: `named_after` the stem,
  `listed` for each name beside the track or in a lyric folder, `placing` answering a `Placing`:
  `Fresh`, `Over`, `AlreadySaid`, `Stands`, `Outranked`), so the desktop's walk of a folder and a
  phone's listing of a granted folder refuse and replace alike; `KeptSheet::ending` and `body` say
  the name's ending and the text, `lyric_folders` the folder names (`WITHIN`).
- **The write is staged and renamed over.** `.<name>.<pid>.resonate-staging` beside the track,
  `Held` from creation and synced, then renamed onto the sheet: `Standing::LeftAlone` by
  `unreplacing::renamed_over_nothing`, which refuses an existing name atomically (`SidecarStands`),
  `Replaced` by a plain rename, so a sheet that lands meanwhile is never overwritten
  (`a_sheet_that_lands_while_the_write_is_staged_is_never_renamed_over`); a dead writer's staging for the same sheet is swept by the next
  write (`staged_for`, `writer::writer_of`;
  `a_staged_sheet_a_dead_writer_left_is_swept_by_the_next_write`).
- **The tags are the second home, through the retag** (`library.md`'s *Fetched lyrics go into a
  file's tags only when the listener asks*): the kept LRC or plain text, read back as `Embedded`
  reads it, put back by `tag --undo` or *Put the last run back*.
- **Only a file of its own has a sidecar of its own**: a row cut from a shared file (a span) or
  any location that is not local is passed over by both callers, the terminal's `tag --lyrics`
  (`binary.md`) and the window's *Save the lyrics* (`ui.md`).
- **The phones save the same way, always as an `.lrc`** (`mobile.md`'s *Saving lyrics beside a song
  or into its tags*): `save_lyrics_beside` writes the kept LRC text (never the Lyricsfile, since a
  phone's scan reads back only `.lrc` beside a song) under the same refusals, and
  `save_lyrics_into_tags` is the retag's lyric write, put back by the phones' tag undo.

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
- **Text a writer puts after a stamp is escaped with `escaped_for_lrc` only where the reader would
  mis-take it, and `unescaped` undoes exactly that.** A text is escaped (one backslash in front)
  when its leading bracket opens what the reader takes (a moment, an id tag, a `[v1:`/`[v2:` voice
  marker, or a marker ahead of a stamp, all decided by the reader's own `Bracket::read`,
  `heads_a_timed_line` and `voiced_text` through `is_taken_by_the_reader`) or when it already leads
  with backslashes then `[`. `[Chorus] x` stays as it is on disk, read as text anyway, and a
  hand-written `\[Chorus]` is left as written. So `[offset:30000] x` written by Better Lyrics'
  `lrc_of` reads back as that text, not as an offset
  (`a_line_led_by_a_bracket_the_reader_would_take_is_read_back_as_text_once_escaped`,
  `a_marker_written_by_hand_is_text_and_a_backslash_before_it_is_left_as_written`).
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
- **A line with no stamp among stamped lines is kept as an aside.** `Reading` notes how many timed
  lines precede each (`plain`), and `finish` interleaves them back in file order
  (`an_untimed_line_among_timed_ones_is_kept_where_it_was_written`); a sheet with no stamp at all is
  unsynced as before. A multi-stamp line's aside follows its last moment.
- **Voices: `[v1: words]` / `[v2: words]` after a timed line's timestamp.** The marker is removed
  from the text; none = voice one; an unrecognised bracket stays text.

## The TTML reader

- **Apple's lyric TTML, as Better Lyrics serves it** (`read_ttml`): each `<p>` is a line timed by
  `begin`/`end`, each `<span>` with a `begin` a word (its `end` the word's), text between spans
  joined to the word before; a word sung in syllables (`good`, `bye`) is two timed parts with no
  space between. Whitespace runs
  read as one space. Clock values: seconds, `m:ss.fff`, `h:mm:ss.fff`
  (`a_ttml_clock_is_read_in_seconds_minutes_or_hours`) and every offset-time metric: `h`, `m`, `s`,
  `ms`, `f` frames (against `ttp:frameRate`, 30 unless said, and `ttp:frameRateMultiplier`) and `t`
  ticks (against `ttp:tickRate`, else the frame rate times `ttp:subFrameRate` where a frame rate is
  said, else 1), read by `Clocks` from the root
  (`every_offset_time_metric_ttml_defines_is_read`,
  `frames_and_ticks_are_counted_against_the_rates_the_document_declares`). A `<body dur>` is the declared length,
  weighed by `Lyricsfile::declares_another_length_than`.
- **Singers are the declared agents, in declaration order**: the first `ttm:agent type="person"`
  in the head is voice one, any other person voice two; a group (`v1000`, all together) is voice
  one. A line names its agent on itself or an ancestor. Order of declaration, not of singing: a
  duet whose second-billed singer opens still reads them right
  (`the_second_singer_declared_is_the_second_voice_whoever_sings_first`).
- **What is beside the words is left out**: a span with any `ttm:role` (`x-bg` backing vocals,
  `x-translation`, `x-roman`) is not read (`background_and_translated_words_are_left_beside_the_line`).
- **A line with no `begin` makes the whole set unsynced**; one with no word spans is line-timed.
  Bounded as the Lyricsfile: `LARGEST_LYRICSFILE`, `MOST_LINES`, `MOST_WORDS`, a node limit, no DTD,
  and `MOST_DEPTH` of nested elements, past which the sheet is `Unread::TooDeep` before any walk
  recurses (a 4 MiB document holds hundreds of thousands of levels)
  (`a_document_nested_past_the_bound_is_unreadable_rather_than_overflowing_the_stack`).
