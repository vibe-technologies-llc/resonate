# Audit

High-confidence correctness defects. Read against `9670cbc`. Each item was confirmed in the
code it names. Nothing here is a fix.

The catalog upserts, the enrichment pairing, the organise apply, the vault keep, the DSP
stages and the LRC reader were read in the same pass and did not yield a defect of this
confidence.

## The queue

### A drag between the album and Play Next leaves the playlist badge where it was

`Queue::split` in `crates/resonate-engine/src/queue.rs` rebuilds `order` and `next` and bumps
`revision`. It does not recompute `playing_from`. That field is written only in `rows_changed`,
which insert, remove, load and `forget` call. `move_rows` is the drag, and it assigns the waits
flag from the row landed on before it calls `split`. `Engine::publish` copies `playing_from`
into `PlayerState::queue_stamp` on that revision bump.

`Library::playing_playlist` treats the stamp as the claim that the queue is still that
playlist. Dragging an album row up into Play Next, or a queued row down into the album, changes
who the queue is playing from and leaves the stamp on the previous set. The badge stays. A drag
back does not restore it either, because the stamp never moved. Insert and remove do restamp.
`a_row_dragged_among_the_queued_rows_joins_them_and_one_dragged_out_leaves` shows the membership
change and never reads the stamp.

A reorder that keeps the same rows in `order` has the same stamp either way. Recompute
`playing_from` at the end of `split` the way `rows_changed` does, and assert the drag that
crosses the boundary moves it and the drag back restores it.

## The window

### Changing the search text closes the open album or artist

`LibraryModel::set_query` assigns `selection = Selection::Everything` on every text change.
The header observer in `RootView` calls it whenever the field's text differs from the text the
library is narrowing on, and `dismiss_search` clears that field when escape is pressed and the
box still holds words.

The escape chain is written to clear the words first and leave the scope for a second press:
`dismiss_search` steps back only once the box is already empty, and `.claude/rules/ui.md`
states that a scoped search takes those two presses. Opening an album does not clear the query,
so album-plus-search is a state `select` can hold. The first character typed into that album,
and the escape that was only clearing the words, both take the selection back to the whole
library. The way-back stack is left standing, so a later escape walks back to the page the
album was opened from rather than out of the album.

Stop clearing `selection` inside `set_query`. Callers that mean a library-wide search —
`revise_search`, *Did you mean*, Listen's *Find it* — can select `Everything` themselves.

### A favourite that did not save stays marked for the rest of the run

`LibraryModel::favour` inserts into `favoured` and rewrites the queued row's `named` cache
before `Library::favour` runs. `favours` returns that map entry for the rest of the process.
The map is written in `favour` and read in `favours`, and nowhere else. `edited` on `Err`
toasts and reloads the catalog. The reload rebuilds the favourite sets and does not touch the
map, so the star keeps the value the write did not keep. The next press sends the opposite of
what the catalog holds. The same entry hides a favourite another process changed after this
window touched the row: `written_elsewhere` reloads the lists, and the star does not follow.

On `Err`, drop that key and put the previous `named` track back before the reload. Leave the
map in place when the write lands.

### A failed want, hide or favourite is told as a playlist change

`edited` is the path `favour`, `hide_track`, `forget_delivered` and `want` use. Its `Err` arm
always says it could not change the playlist, and `unwant` goes through `edit`, which shares
that arm. The toast is the only thing the window shows for the failure. The words name a
playlist edit the listener did not make.

Name the operation the call was. The playlist edits can keep the playlist wording.

### The album name's underline runs to the end of the player panel

The album on the playback bar is the one name given `flex_1` and `ends_in_an_ellipsis`.
`TransportView::by_line` in `crates/resonate-ui/src/views/transport.rs` does that after
`opens`. `opens` has already called `truncate()`, so the element still carries an ellipsis
overflow, and `ends_in_an_ellipsis` then sets `whitespace_normal` and `line_clamp(1)`. The
title above it is cut with `cut_to_fit` and kept at its own width. The artist is kept at its
own width. Hovering either of those underlines the glyphs.

gpui 0.2.2 paints that underline to the unwrapped line. `paint_line` in `text_system/line.rs`
ends it at `first_glyph_x + layout.width`, and subtracts a wrap boundary's glyph only when one
was recorded. `compute_wrap_boundaries` with a clamp of one line records none: the moment the
name passes the slot, `boundaries.len() >= max_lines - 1` breaks while `boundaries` is still
empty. The underline is the whole unwrapped run. Once the album is longer than the room left
beside the artist, that run is wider than the glyphs still inside the slot, and `overflow_hidden`
clips the line at the slot's edge. The underline fills the leftover of the panel.

Shape the album the way the title is shaped, at the room it was given, and paint the underline
to that cut. A clamp of one line has to record the boundary it stopped on.

### The names on the player panel are not a right click

`played_title` and `by_line` draw the track, the artist and the album through `opens`. That is
a left click: `opened` scopes the library to the album or the artist. A right click on the
track name, the artist or the album reaches no menu.

The cover beside them has a menu — magnify, the inspector, the artist, the album, favourite,
share — and none of those copies the words. A track row's menu copies the file path
(`offers_the_file`, `COPY_PATH` in `views/menu.rs`) and does not copy the title, the artist or
the album. The window copies text from a lyric line and from the search field. A track name, an
artist and an album name have no copy of their own.

A right click on each of those three names should copy that name. The row menu can offer the
same three beside the path.

### Stopping Listen leaves the button and the prompt out of line

`ListenModel::stop` in `crates/resonate-ui/src/listening.rs` sets `Stage::Idle` while a
recording or a lookup is still running. `listen_stage` draws `Idle` through `listen_again`, and
`Unknown`, `Silent` and `Unreached` take the same function. The prompt is a full-width line.
The button under it is `kit::actions`.

`kit::actions` in `views/kit.rs` is the heading's action row: `flex_none`, `justify_end`, and
a max width of 64% of its parent. In the sheet's column that row stays at the left and the
Listen button sits at the right edge of the 64%. The prompt stays on the card's left edge.
While the sheet is recording, the same column draws the prompt and a bar at `w_full`, and
stopping swaps that bar for the offset button. A found
song puts Listen again, Open and Find it through the same `actions` row.

Draw the Listen button in the sheet's own row, at the prompt's left edge and at the card's
width. Leave `kit::actions` on the headings it was built for.

## The lyrics

### The sheet steps a pixel as its bounce is cut off

A line change glides on `spring` in `crates/resonate-ui/src/lyrics.rs`. The damping is 0.8, so
the sheet travels past the sung line and comes back: the peak is about 1.5 % past the landing,
near 520 ms. `spring` then returns 1.0 the moment `GLIDE_SETTLES_IN` (820 ms) is reached. The
curve has not arrived there. A millisecond before the cut it is still about 0.13 % long, which
on a glide of a few hundred pixels is about one pixel, and on a long one a pixel and a half.
`Glide::at` writes that value straight into the scroll offset, and `lag_of` uses the same clock
for the lines below the sung one, so they step with it.

The frames keep coming after the cut — `settled` waits out the last line's lag — so the step is
drawn on its own, after the bounce has already come back. `a_spring_runs_from_rest_to_rest`
holds the value at 820 ms to exactly 1.0 and allows the overshoot before that, which is this
cut.

Let the spring run until it is inside half a pixel of the landing, and take the offset from the
curve the whole way. The lines' lag should read that same curve.

## Playback

### A seek near the start of Opus in Matroska skips the pre-skip short

`Carrying::OpusInMatroska` sets `music_at` to timestamp zero, because symphonia has already
subtracted `CodecDelay` from the block timestamp. A cold open then skips `playable.start()`,
which is the OpusHead pre-skip in samples: 312 at 48 kHz, 6.5 ms.

A seek does not. `restart` skips `Landing::short_of_the_music`, and that converts the landed
timestamp through the segment timebase. Matroska's default scale is 1 ms. symphonia's
`MatroskaTicks::into_track_ticks` divides the delay in nanoseconds by that scale, so
6_500_000 ns becomes 6 ticks. Six milliseconds at 48 kHz is 288 samples, 24 short of the
pre-skip. `opus::pre_roll` pulls every seek in the first 400 ms back to the start of the
stream, so the seek lands on that first packet. `limit` stays the exact playable length, and
the decode stops 24 samples before the end as well.

`a_seek_into_opus_hears_what_decoding_from_the_start_hears_there` seeks to one second, past the
pre-roll. `a_seek_landing_ahead_of_the_music_does_not_hear_the_priming` does not open an `.mka`.

When the landed packet is the start of the stream, discard `priming()` samples. Leave Ogg on
the sample-accurate timestamp. A short frame whose timestamp is negative and is not the first
packet still has to use the tick delta, or the skip eats music. Add a seek to zero, and one
inside the first 400 ms, against the `.mka` the Opus tests already build, compared with a
straight decode.

## Imports and the bus

### A scan asked over MCP keeps a folder before it has accepted the whole list

`Passes::scan` in `crates/resonate-mcp/src/passes.rs` calls `Library::add_root` inside the loop
that checks `is_dir`. `add_root` commits that root on its own. A later path that is not a
directory returns `Error::NoSuchFolder` and never starts `library.scan`. `.claude/rules/mcp.md`
says a path that is not a folder is refused before anything is kept.

`start_scan` of `/music/real` followed by `/music/missing` answers a failure, and `/music/real`
is a root. The next bare scan and the window's watch walk it.

Stat every path first. Register them only once the whole list is folders.

### A cover the bus could not finish writing is published on the next request

`lay_down` in `crates/resonate-mpris/src/art.rs` opens the cover with `create_new` and then
`write_all`. A short write leaves the partial file and returns the error. The caller does not
cache that attempt. The next request for the same bytes hits `ErrorKind::AlreadyExists`, and
that arm returns `Ok(())` without reading the file back. The URI of that path is then what
`mpris:artUrl` and the track notification name.

Write to a staging name, sync it, and rename it into place. Remove the partial file on any
error. Treat a file that is already there as the picture only when its length is the length of
the bytes just hashed.

### A playlist file URI keeps its query and fragment in the path

`sheet::local_file` percent-decodes everything after the authority. It does not stop at `?` or
`#`. M3U, PLS and XSPF `file:` locations all go through `sheet::from_uri`, so
`file:///music/Echoes.flac#t=10` and `file:///music/Echoes.flac?at=10` are stored as paths that
end in `#t=10` and `?at=10`. A relative XSPF location takes the same decode in
`xspf::referenced`, so `Echoes.flac#t=10` is that filename. The row counts as missing and does
not play. This build's own `#frames=` URI, pasted into a sheet, is the same shape.

`MediaLocation::from_uri` already keeps the path before the first `?` or `#`. No playlist test
covers either delimiter.

Cut the reference at the first raw `?` or `#` before unescaping, in `local_file` and in the
relative XSPF path. `%23` and `%3F` still decode to a literal `#` or `?` in the name. A plain
M3U or PLS row is not a URI: a `#` in the middle of it stays part of the filename.

### Discord shows one cut's cover for another cut of the same file

`cover_for` in `crates/resonate-discord/src/publish.rs` returns the cached cover when the track
id and the location match and a cover was found. `CachedCover` stores neither the span nor the
release the cover was resolved from. A hit never calls `tagged` or `Releases::cover` again.

`Unclaimed` mints from `TrackId::MAX`, and a new load mints again from there. Play one cue cut
of `album.flac` as the first unscanned row, then load another cut of that path as the first row
of a new queue. Both rows are `TrackId::MAX` at the same path. The cache hits, and Discord keeps
the first cut's cover for the rest of the process. Two cuts whose tags name different releases
are the case that shows the wrong sleeve. A library row has its own id and does not take this
path.

Store the span beside the id and the location, and treat a different span as a miss.
