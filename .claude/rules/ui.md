---
paths:
  - "crates/resonate-ui/**/*.rs"
---

# The window

`resonate-ui` is GPUI on Wayland. Depends on engine, library, lyrics, `resonate-listen`; not on
`resonate-codec` or image decoding (`image` only for the frame gpui's `RenderImage` is built of). A
`Reference` arrives only as the trait object the binary hands `run` in `Lookups`; the online crate
is never named.

## Chrome and input

- **Title names what plays.** `RootView::name_the_window` (in `render`, from cached `Playing`) sets
  *Echoes — Pink Floyd · Resonate* (artist omitted if blank), else *Resonate* (nothing playing,
  blank title); only when the text moved; unchanged between songs
  (`the_window_is_titled_by_what_plays_and_by_the_app_where_nothing_does`).
- **The window is its own titlebar.** `WindowDecorations::Client`; the header (wordmark, search
  field) draws minimise/maximise/close, drags the window, opens the compositor's window menu.
  - Header: `theme::header_height()`, three columns; wordmark and controls each take sidebar width
    less padding, so the field centres on the window, at most `theme::search_width()`. Listen button
    at the field's right edge in the middle column (one group).
  - Control = `theme::WINDOW_CONTROL` (31) square (odd: the odd mark centres on a whole pixel, not
    rounding low); row pulled right by `mark_inset` (control edge to mark) so the close mark stands
    in the wordmark square's gutter. *Resonate* drops `WORDMARK_CAPITALS_CENTRED_BY` below its line
    box, centring capitals on the accent square (descender space leaves them high). Empty unfocused
    field draws `ctrl-f` in the mono face at its end (the one key taking focus deliberately is
    otherwise nowhere on screen).
  - `views/chrome.rs` owns the frame: `theme::RESIZE_BORDER` gutter outside the painted window
    (resize edges, shadow); rounded corners on non-tiled edges.
  - **Shadow is cast by a rim, not the window.** gpui shades a shadow over its whole box, three
    blurs past it: on the window, blur arithmetic ran over every pixel every frame for a fringe a
    few pixels wide (a quarter of a tracks-pane frame's GPU cost). `shadow_rim` casts the same
    `BoxShadow` from four strips under the content along its edges, each as deep as the blur
    reaches, top/bottom carrying the corner rounding; blur is linear, so the four sum to the whole
    box's shadow outside the window (pixel comparison: no difference past 2 in 255).
  - **Gutter repaints only where the resize edge under the pointer changes**: `resize_cursor` sets
    the cursor from the pointer in paint (`frame` once `refresh`ed the window, dropping every cache,
    on every pointer move over it).
  - **gpui clips a child to its parent's rectangle, not its rounding**: header and playback bar
    (`surface` fill to the corners) painted a square past the rounded border. `chrome::rounded` =
    the one reading of which corners are round; `rounded_within_the_frame` = same less
    `FRAME_BORDER`, used by header (top corners) and playback bar (bottom), so fill nests in the
    border.
  - **An edge held to the screen is not a resize edge.** Tiled edge loses its gutter (content runs
    to it); `grabbed_edge` reading every edge's outer `RESIZE_BORDER` turned a press on a maximised
    window's playback-bar bottom strip into a compositor resize grab that ate the click. It weighs
    only edges `held_edges` leaves free: none while maximised/fullscreen, whatever tiling the
    compositor reported (Zed's rule; `chrome.rs`'s tests). gpui forces every edge tiled then,
    dropping gutter, rounding, shadow.
  - Compositor may refuse client decorations (gpui reports `Decorations::Server`): controls and
    drag handlers render only under `Decorations::Client` (no second button set).
  - **Which of minimise/maximise draw weighs two readings**: `Window::window_controls` (compositor
    offers) and `ResonateApp::window_buttons` (`WindowButtons`, filled by `minimise-button` /
    `maximise-button` keys, written by Appearance's *Window buttons* switches).
    `RootView::window_controls` draws a button only where both say yes: a setting removes one,
    never conjures one a compositor withheld. The switch writes the global and notifies the root
    view (redraw next frame, not next run). Close is not a setting (the pointer's one way out); a
    double press on the bar still zooms whether or not maximise draws (`chrome::titlebar` never
    asks).
  - `window_min_size` = where the header's children stop clipping, not a size panes were designed
    around; a compositor may ignore it.
  - **All four marks are icons, not glyphs.** `Icon::WindowMinimise`, `WindowMaximise`,
    `WindowRestore`, `WindowClose`: SVGs with strokes to the viewBox edge (`Icon::Close` is inset),
    stroke = the 24-unit box's share of one device pixel at drawn size, as crisp as the bordered
    `div`s they replaced, lit on hover via `icons::lit_on_hover`. All four are `theme::WINDOW_MARK`
    less its overlap across (one size, one centre line). A glyph could not: no box in the primary UI
    face (maximise always needed drawing), and − × centre on the math axis, leaving × half a pixel
    off the square beside it at half its width.
- **Closing removes the window; quitting follows, never the reverse.** Close control, `ctrl-q` and
  the bus's `Quit` call `Window::remove_window`; `app.rs` quits once the last window closed.
  `App::quit` under a live window cleared gpui's window map while the compositor held the surface:
  the pointer leaving the pressed close button reached a missing window, gpui logged *window not
  found* twice every exit. Removing first drops platform window and callbacks; Wayland's client
  stops the loop when its last window goes.
- **The window names itself to the compositor twice.** `WindowOptions::app_id` = `APP_ID`
  (`xdg_toplevel.set_app_id`), matched against `resonate.desktop` for icon and grouping; the entry's
  `StartupWMClass` is held to it by a test in `app.rs`. The caption is a second call (gpui's
  Wayland backend never reads `WindowParams::titlebar`; `TitlebarOptions::title` names only the
  painted header): `Window::set_window_title` on open names the taskbar/switcher entry.
- **Everything clickable in the header stops the press.** `chrome::titlebar` puts
  `start_window_move` on the whole bar, so a reaching mouse-down is a drag: window controls and the
  search field carry `on_mouse_down(Left, stop_propagation)` (else unclickable).
- **The search field is a real text input.** `views/field.rs` owns `Field`; `edit.rs` the gpui-free
  `Edit` (content, caret, selection): caret, word motions, grapheme steps test windowless.
  - **A word is a run of graphemes of one class** (space, letters/digits, symbol), classed by the
    grapheme's first character (a decomposed accent stays with its letter); letter runs further
    break at UAX #29 word boundaries (`Edit::runs`): unspaced Han/Hiragana moves a character at a
    time, a katakana run is one word.
  - View half = custom `Element`: shapes the line, paints selection and caret, registers an
    `ElementInputHandler` (IME preediting, `bounds_for_range` for the candidate window, caret
    to/from pointer). The line scrolls under the field to keep the caret in view (no truncation). A
    selection drag is followed from a `Window::on_mouse_event` registered in `paint` (the element's
    `on_mouse_move` fires only over it; why `views/slider.rs` has a drag surface).
- **TIDAL is signed in to from its group, never by pasting a token from elsewhere.** The *A TIDAL
  account* group holds `RootView`'s `SigningIn`: `TidalSigning::Idle`/`Asking`/`Waiting` (with the
  `Authorizing` shown), a stop flag, the task. `sign_in_to_tidal` runs the `SignsIn` seam's two
  calls on the background executor; the code shows as a `kit::figure` beside *Open the page*
  (`cx.open_url`) and *Stop* (sets the flag the waiting call reads). The token lands in the
  refresh-token field, the global and the settings file at once; a refusal is a toast naming which
  (`providers.md`: the seam). hifi-api and Monochrome addresses are optional custom-server
  overrides: blank = hosted service; submitting blank says so.
  - **An account given is asked at once.** Every field of the *A Subsonic server* and *A TIDAL
    account* groups writes the global `Online` as it writes the file; the registry is built from it
    (`Sourcing::providers`), so a sign-in, server or password applies on the next poll.
    `the_sources_moved` calls `LibraryModel::sources_moved`: wherever a provider is now registered,
    every *No provider is set up* download returns to *Queued* and every unheld want is asked, as
    *Poll now* does
    (`a_song_asked_for_before_any_provider_was_set_up_is_fetched_once_one_is`); turning Online on
    does the same.
  - A server address (Subsonic, custom hifi-api, custom Monochrome) must begin `http://` or
    `https://` with a host after it (`reads_as_a_server`); else a toast, nothing stored (bare
    `music.local:4533` once failed every poll silently).
- **A secret is drawn as marks.** `Field::masked` (Subsonic password, TIDAL client secret and
  refresh token, AcoustID/AudD/ListenBrainz keys) shapes one `•` per letter; `Shown` maps every
  content offset (caret, selection, marked range, IME bounds) to the drawn line and a pointer back:
  editing is a plain field's. Copies and cuts nothing
  (`a_masked_field_draws_a_mark_a_letter_and_maps_every_offset_both_ways`).
- **The caret blinks as the desktop says.** `CaretBlink` = shared half-period on `Stored` and
  `ResonateApp` (`as_built` 500 ms; `steady` holds it lit); every field's blink task rereads it each
  tick (late answers apply next tick; a held-still caret looks again a second later). Binary's
  `caret_as_the_desktop_blinks` asks on a `resonate-caret` thread (window never waits):
  `resonate_mpris::caret_blinking` reads the Settings portal's `ReadOne` under a one-second
  deadline: `org.gnome.desktop.interface`'s `cursor-blink` and `cursor-blink-time` (KDE's portal
  answers too), else `org.kde.kdeglobals.KDE`'s `CursorBlinkRate`. Cycle of nothing, or blinking
  off = steady; no answer leaves the half second; cycle halved (dark and lit together)
  (`the_caret_blinks_as_the_desktop_says_and_holds_still_where_it_says_not_to`).
- **Search undo steps are spans of edits, not keystrokes.** `edit.rs` keeps a bounded stack of
  content-and-selection snapshots, coalescing consecutive edits of the same `edit::Span` (`Typing`,
  `Removing`, `Composing`; not `resonate-core::Span`) *and* resuming from the selection the last
  left: a typed word, a backspace run, an IME revising a preedit are one step each. Paste, cut,
  clear are `Span::Discrete`, never join (`ctrl-a` over the lot, and escape, stay recoverable).
  Whitespace closes a typing run (undo walks back a word at a time).
  - **An edit changing nothing is no step**: `Edit::edited` weighs the range's content against its
    replacement; backspace at the start, delete at the end, pasting what is selected only move the
    caret (no snapshot; what was undone stays redoable); `set_content` with the present text
    likewise.
  - History is `Edit`'s (windowless tests): 128 steps (`UNDO_DEPTH`) of content at most
    `LONGEST_CONTENT` (16 KiB); a typed, pasted or composed run is cut to fit at a character
    boundary (`a_paste_longer_than_a_field_holds_is_cut_to_what_fits`; a field once took a megabyte,
    reshaped it every blink, kept it whole per snapshot). Lives only as long as the run: escape
    clears and blurs in one stroke, so what it threw away is out of reach until a click returns the
    caret. The three keys bind under `SEARCH_CONTEXT` (a blurred field never sees them); outside
    the field they walk a playlist edit, so what they reach depends on the caret.
- **The window names itself; a negated binding is dead without it.** Root div carries
  `key_context(WINDOW_CONTEXT)` beside `track_focus`: stack `["Resonate"]` at rest,
  `["Resonate", "Search"]` in a field. gpui's `KeyBindingContextPredicate::depth_of` returns `false`
  for an *empty* slice before reaching the `Not` arm, so an unnamed window disables every `!Search`
  binding (transport, reach, undo, redo) while the field's own work (the one element naming
  itself): the asymmetry is the tell. `app.rs`'s tests pin both stacks against the predicates, one
  asserting gpui still refuses the empty slice (a toolchain changing it says so).
- **An opened album/artist stands under its category; pressing the category again leaves it.** The
  page is `Pane::Tracks` narrowed to a `Selection`, but one opened from the albums grid is *in
  Albums*: the sidebar marks `RootView::in_front` (`in_front_of`: pane + selection) = Albums for an
  album, Artists for an artist, else the pane. A sidebar press is `RootView::choose_pane`; `landing`
  is the whole decision (windowless tests): the row already in front returns to its whole category
  (clearing the scope or closing an opened playlist); a category whose page still stands behind
  another pane returns to it (as a tab keeps its place); *Tracks* always lists every track (a scope
  never belongs to it); else only the pane changes. Before: sidebar marked *Tracks* with an album
  open, pressing *Albums* left the scope to surface later under *Tracks*, pressing *Playlists*
  inside a playlist did nothing.
- **`ctrl-tab` walks the sidebar; browse panes answer the reach keys.** `NextPane`/`PreviousPane`
  step `Pane::BROWSE` from `in_front`'s pane via `choose_pane`, wrapping; an unlisted pane steps
  onto the first listed (`stepped_pane`, windowless like `Step::landing`).
  - `Shift::Listing` names a browse listing by `Listed`: tracks and artists first, each growing a
    `UniformListScrollHandle` so `show_row`'s centred scroll works; a reached row wears the queue
    row's accent edge. A listing refuses what it has no order of its own to change
    (`Shift::is_edited`): `delete`, `alt-up`, `alt-down` dead there; reach, page keys, `enter` not
    (`enter` plays a track or opens an artist, as a click).
  - `ctrl-shift-left`/`ctrl-shift-right` join the media keys in `answering_anywhere`: step the queue
    wherever the caret is not in a field; in one, `field.rs`'s `SelectWordLeft`/`SelectWordRight`
    under the field's context outrank them (extending a selection never skips the track).
- **A search is left for its results from the keyboard; the albums grid is reached too.** In the
  search box `down`, `tab`, `enter` = `GoToTheResults`, `TabOnward`, the field's `Submitted`, all
  landing on `RootView::go_to_the_results`: caret returns to the window, the first row of what the
  pane lists is reached; next `down` steps through results, `enter` plays/opens. `tab` binds under
  `SEARCH_CONTEXT` to outrank the window's; from any other field `tab_onward` is `focus_next`. A
  query that moves drops a reach left in a listing (its rows are no longer the ones it was put on):
  `down` after typing starts at the top
  (`down_and_tab_in_a_field_are_the_fields_own_before_they_are_the_windows` pins the bindings).
  - `Listed::Albums`: reach keys step albums in reading order, a page = as many whole grid rows as
    shown; `show_row` scrolls the grid row holding the album; `enter` opens it; reached cell wears
    `reached_ring` (accent border over the cover, costing no room).
  - *Top results* is one run, `Listed::Top`, in the order the page draws it: held artists
    (`STRIP_AT_MOST`), artists not held, held songs (`SONGS_AT_THE_TOP`), songs found beyond the
    library (`FOUND_AT_THE_TOP`), held albums, albums not held. `search::TopRun` is the arithmetic
    (`at` a row's `TopEntry`, `row_of` an entry's row, `len`); `RootView::press_at_the_top` presses
    what `enter` reaches as a press would (open the artist or album, land and open one not held,
    play a song, want a found one); a reached cell wears `reached_ring` round the whole cell
    (`at_the_top`). `show_row` sets `reached_unseen`, and the reached element's
    `kit::brought_into_view_within` moves the page's scroll down and the strip's across
    (`shelf_scrolls`) just far enough
    (`the_artists_songs_and_albums_on_the_top_results_are_one_run_the_keys_walk_and_press`,
    `every_found_song_below_the_held_ones_can_be_reached_from_the_keyboard`).
  - `Listed::Favourites`, `Missing`, `Suggested` carry the reach into favourite tracks, Missing's
    rows, an opened suggestion's rows: `enter` plays a favourite/suggested track; on a Missing row
    opens the album it is short of or the artist whose release it is (a disc heading answers
    nothing). Missing's two tabs are two listings under one `Listed`, so switching lets go of the
    reach (`let_go_of_the_reach_in`). `reorder::marked` is generic over what it marks (a Missing row
    is a plain `Div` in its card). All three are narrowed by the search: typing at them searches.
  - `Listed::Offered`/`Heard` reach the two non-`uniform_list` panes: Suggestions' cards in shelf
    order (`in_shelf_order`, kind by kind; `enter` opens the card), Statistics' three tables as one
    run, tracks, albums, artists (`enter` plays the most-heard tracks from the reached one, or opens
    the album/artist). No `scroll_to_item`: `show_row` sets `RootView::reached_unseen`, the reached
    element carries `kit::brought_into_view`, a canvas that on next prepaint moves the pane's
    `ScrollHandle` just far enough and spends the flag (a later wheel turn is not fought). A page =
    what the viewport holds (whole card rows, or `row_height` rows); choosing another statistics
    window, like opening a card, lets go of the reach.
- **Media keys are a fallback, not the feature.** A desktop grabbing transport keys consumes them
  and calls MPRIS (the intended path; works with the window behind everything); where a session
  grabs none, the key reaches the focused window and the binding answers. gpui names an unmapped
  keysym by lowercasing `xkb_keysym_get_name`: `xf86audioplay` (the one play/pause key most
  keyboards carry; toggles), `xf86audiopause` → `Pause`, `xf86audiostop` → `Stop`, `xf86audionext`,
  `xf86audioprev`; `XF86Back`/`XF86Forward` gpui does map, to `back`/`forward`. All seven are in
  `answering_anywhere` (a transport-row key must not stop because the caret is in the search
  field). `app.rs`'s test pins the seven names against `Keystroke::parse` (a name gpui cannot read
  binds to nothing, silently).
- **Typing at a list no search narrows jumps through it.** The search narrows the tracks, albums,
  artists, playlists-index and opened-playlist panes: typing there means search. The queue narrows
  on nothing: `RootView::typed` routes a keystroke there to `TypeAhead` before the search arm.
  `jumped` prefers a row *starting with* the letters over one holding them, wraps at the end;
  `RootView::jumping` is the one place saying which panes opt in and how to read their rows;
  landing = the reach keys' `reach_at` and centred `show_row`.
  - Pill says **JUMP TO** or **NO MATCH** in words (a colour is unreadable in a screenshot; a failed
    jump must be legible); no id, no listeners, so gpui inserts no hitbox and it blocks nothing;
    held `HELD_FOR` (1100 ms). `typing.rs` holds the gpui-free arithmetic (tested as `Following` and
    `Step::landing`). Folds through `resonate_library::folded_letters` (search box's and
    `artists.key`'s fold): *przybylowicz* reaches *Przybyłowicz* at the queue as in the search
    field.
  - **Names a jump reads come off the render thread.** `QueueNames` holds the queue's titles
    against a `NamedAt`: the `Queued::revision`, the catalog's `Library::names_stamp` (moving only
    where a title, artist, album or genre is written, or a row arrives or goes: a counted play, a
    favourite, a playlist edit move nothing) and `Player::media_revision` read at, so a retagged or
    enriched queued track is found by its new name. `names_in_the_queue` hands them out while all
    three stand (`CatalogStamp::still_holds_at`); a name moved or another media revision asks again
    but still hands out what it holds for the same rows meanwhile; another
    queue revision hands out nothing. Asking is the background executor: `queued_rows` reads by id in one
    `Library::tracks_with_ids` pass (500-id batches; a row whose id names another file falls to its
    path), then `Player::media`, then the stem, in queue row order. Each name is held folded
    (`Named`: the row, the catalog row it was read from and its `folded_letters`), so a key weighs
    strings, never folds one on the UI thread. `named_again` keeps every row already named and
    reads only those the queue edit brought (`a_queue_edit_names_only_the_rows_it_brought`) and
    those `Library::names_moved_since` the names were read says a write retitled (by the catalog
    row a name was read from; a row the catalog did not hold only where a row arrived): a scan
    writing batch after batch under an open queue pane reads the rows it touched, not every queued
    row per batch (`a_name_written_in_the_catalog_reads_again_only_the_rows_it_retitled`). Another
    media revision still reads every row. A keystroke before they land
    still counts as a jump (`jump_where_typed` makes it once they do). The queue pane asks as it
    draws, so names are usually ready before the first letter (once read on the first keystroke on
    the render thread, up to two SQLite reads a row through a cache smaller than a long queue).
- **Backspace drops a typed letter before a reach, and never the row a jump landed on.** A jump
  *sets* the reach, so the other order made backspacing a mistyped letter take out the row just
  landed on. `drop_typed` answers false when nothing is live and the pill lapses after `HELD_FOR`,
  so a typo noticed a second late would still take the row: `jumped_to` notes the reach it set in
  `landed_by_a_jump`; while the reach is still that one in the pane in front
  (`stands_where_a_jump_landed`) backspace does nothing. Any reach made by hand (reach keys via
  `reach_by_hand`, a press via `reach_at`, a move, a drop) forgets it, so a deliberately reached row
  backspaces away as ever; `delete` takes a jumped-to row (no typing key). Escape clears a live
  type-ahead after the magnified cover and the notice, before `dismiss_search` (those two stand
  until taken down, this lapses after a second; abandoning a jump is not asking to clear the
  search).
- **Typing anywhere searches; the letter that starts a search puts the caret in the field.**
  `RootView::typed` hands a letter no jump takes to `type_where_typing_goes`: focuses the search
  and appends it, so later keys are the field's and escape leaves and clears as for a clicked-into
  search (appended unfocused, escape was spent ending the typing and the field kept nothing of its
  keys).
  - `space` at the window = `PlayPauseUnlessTyping`; `is_typing` makes it a typed space for
    `HELD_FOR` after the last letter or while a jump is live: *pink floyd* typed during a jump is a
    search, not a pause. The hardware play key keeps `TogglePlayPause`, which never types.
  - Stop, shuffle, repeat = `ctrl-s`, `ctrl-h`, `ctrl-r`: bare `s`/`h`/`r` fired on every letter of
    a search (typing *Rush* stopped the music;
    `typing_at_the_window_searches_with_every_letter_and_its_spaces_rather_than_steering`).
  - Bound under `!Search && !Control`: `space`, the three, `left`, `right`, `ctrl-left`,
    `ctrl-right` (the transport would otherwise take them from the caret), reach, move, undo, redo
    keys, and the four reaching what was once the pointer's alone: `shift-right`/`shift-left` seek
    `SEEK_FURTHER_SECONDS` (30; plain arrows five), `ctrl-m` = the speaker's `toggle_mute`, `ctrl-u`
    = the queue button's `toggle_queue` (control-letters and shifted arrows: a bare letter would
    make the search untypeable).
  - An action fires before any `on_key_down` listener and stops propagation; a keystroke reaching
    neither goes to the platform input handler, so a key bound with no predicate never reaches the
    query.
  - **Which predicate a key carries is structural**: `answering_anywhere` (none),
    `answering_away_from_a_field` (`!Search && !Control`), `answering_where_the_caret_is` (control
    context), `answering_on_a_band` (`BAND_CONTEXT`, the equaliser's) are four lists; `bindings`
    concatenates them with `field::bindings`. A new key goes in one;
    `every_key_not_named_to_answer_anywhere_is_dead_while_a_caret_is_held` walks the second and
    holds each to a predicate dead in a field and on a control and alive at rest. It is what `up`
    and `down` lacked (volume, no predicate, moved while a name was typed): volume is now
    `ctrl-up`/`ctrl-down`, plain arrows move lists. Editing keys live in `field::bindings` under
    the same context.
  - Escape blurs and clears, closing the naming row before clearing a search; a press outside
    blurs via `on_mouse_down_out`. `ctrl-f` takes focus deliberately, so no predicate: it reaches
    the field from inside too and selects what is there (next keystroke replaces the query).
- **Escape away from the field is a chain, and a notice is in it.** `RootView::typed` hears escape
  outside the search field (`LeaveSearch` binds under `SEARCH_CONTEXT` alone: an action fires before
  `on_key_down` and stops propagation, so also binding it under `!Search` would take the key off
  the handler answering it). Order = the match arms, over-the-pane before beside-it: delete sheet
  (`keep_the_track`), Listen sheet, magnified cover, playlist picker (`stop_naming`), shown record,
  open menu, downloads panel, standing toast (`toast::dismiss`), live type-ahead (`stop_typing`),
  then `dismiss_search`: close the naming row, clear the query, or (query empty)
  `RootView::step_back`, the page's way-back seam (history if any, else the in-front category). That
  last step makes a scope escapable (opening an artist from the tracks pane narrows it; the only way
  back was a heading button). Innermost out: a scoped search takes two presses, words then scope.
  - That holds only because typing is not a gesture on the scope: `LibraryModel::set_query` leaves
    `selection` alone (a word typed into an open album narrows the album); callers meaning the
    whole library (`revise_search`, `search_instead`, used by *Did you mean*, *Sung in*, a
    suggestion's *Search for these*, Listen's *Find it*) call `show_everything` before writing the
    words. A toast click is the same `toast::dismiss`.
  - The caret keeps escape for itself: a focused field never reaches the match (`editing` returns
    first). `editing` asks `RootView::text_fields`, the one list of every `Field` the root holds
    (Subsonic and TIDAL accounts included), so none is left out as the ListenBrainz token once was
    (every letter typed into it also landed in the search;
    `what_is_typed_into_the_listenbrainz_token_stays_out_of_the_library_search`). Each field still
    needs its own `dismiss_search` arm; the AutoEq search's `leave_looking` clears it and its
    results and hands focus back (escape there once cleared the library search or stepped back a
    pane; `escape_in_the_autoeq_search_clears_it_and_leaves_the_pane_where_it_was`).
- **What stands over the pane holds the pane's keys back.** A menu, playlist picker, magnified
  cover, record card, Listen sheet or delete sheet is `something_stands_over_the_pane`; meanwhile
  reach, page, widen, move, `enter`, `delete`, undo, redo and every typed letter/backspace answer
  nothing (focus returns to the window wherever the pointer or a closed field leaves it; else the
  pane behind was edited through the sheet). The check sits in each gesture, not the bindings, so a
  button or future caller is held back too; `reach_row` and `play_reached_rows` step and press the
  menu first (it answers those keys itself). Escape is the one key still heard, taking the sheet
  down (`views/root.rs`'s driven tests hold each sheet).
- **A slider is grabbed on the press and followed from a window-wide surface, not the rail.**
  `views/slider.rs` owns both rails: the press starts a `Grab`; `drag_surface` (an absolutely
  positioned child of the app, occluding while held) carries move/release listeners (a rail's own
  `on_mouse_move` fires only over those few pixels). It must be a *child*: `chrome::frame` adds its
  own `stop_propagation` move listener to the app element and one element's listeners run in
  reverse order, so anything registered beside it in `render` never sees a move.
  - Volume applies on every move; the `VOLUME_SETTLE` debounce keeps a drag from rewriting
    `config.toml` per pixel. A seek only previews and commits one `Command::Seek` on release (a
    scrub costs one gap), and only if the press's track still plays (a `Grab` carries its
    `TrackId`; `a_seek_rail_released_after_the_track_changed_seeks_nothing`).
  - On release `PlayerModel::seek` holds the requested position for the playback bar until its
    `Outcome` answers (not the last poll's position between preview and landing). The engine
    publishes before answering and the model polls the answer before reading published state, so
    dropping the held position and taking the new one happen in one refresh. A newer seek replaces
    the held position, a changed track clears it, a refused seek restores the published position
    and raises the ordinary command toast. Only `shown_position` reads the held target: `state`
    stays what the engine published (lyrics, play counting, queue resumption).
  - Wheel over the volume cluster = a `VOLUME_A_NOTCH` (touchpad pixels counted in
    `PIXELS_A_NOTCH`) while `ResonateApp::scroll_volume` says so; `RootView::volume_aimed` is what a
    run of notches or held keys adds to (the engine publishes the new volume a poll later; reading
    it back each notch lost all but one per poll).
  - **A press on the speaker, or `ctrl-m`, mutes; mute is not a volume.** `RootView::toggle_mute`
    sends `Volume::MUTE` and keeps the heard level in `muted_from`, storing nothing (a run quit
    muted opens at the chosen level). `muted_at` reads the pair only while the engine publishes
    mute, so a volume set over the bus ends the mute; a second press, a wheel notch, `ctrl-up`,
    `ctrl-down` restart from the kept level; grabbing the rail is a volume of its own and forgets
    it. Where the device has the slider (`OutputStatus::device_turned`) the press is the device's
    own mute: `Command::SetDeviceMute`, flipping `device_muted`, so a balance set in the desktop
    mixer survives and the desktop's mute switch agrees with the mark (`audio.md`).
  - Release is taken from three places (a release inside the window, `on_mouse_up_out`, a move
    reporting no button held): a pointer leaving mid-drag is otherwise never told to let go. The
    equaliser's handles ride the same surface: a `HeldBand` beside the `Grab` makes it occlude and
    follow; one release lets either go (`eq.md`: the curve's side).
- **A drag leaving the window stops being followed (compositor's line).** gpui reports motion only
  inside the window: a slider dragged past the edge freezes at the last inside value and lets go on
  the way back (no tracking to the rail's end); a text-selection drag tracks anywhere inside, stops
  at the edge. `on_mouse_up_out` still hears the release; nothing hears the motion.
- **`PlayerModel::state` returns a borrow of the model, not a copy** (held as long as `cx` is
  borrowed). Read the model once, clone what is needed, before anything taking `&mut Context`:
  `let model = self.player.read(cx); let state = model.state().clone();`. Inline
  `self.player.read(cx).state()` then `now_playing` or an entity's `update` is a borrow error.

## The design system

- **`theme.rs` = palette and scale; `views/kit.rs` = the vocabulary every pane is built from.** A
  palette is a `Flavour`: surfaces `background`, `surface`, `raised`, `hover`; `border`, `outline`
  between them; `text`, `muted`, `faint` over them; `pitch`, `paper` (either may sit on an accent
  fill); `scrim` (under a sheet); `alarm` (close control's hover); seven `Accents`; `native` (the
  accent it was built around). Worn flavour, accent and `TextSize` = `resonate_core::Appearance`
  (`Theme`, `Option<Accent>`, `TextSize`: names with a text form, no hex; in core because the
  config reader, built without `ui`, and the window both need it, like `MediaLocation::from_uri`).
  Its `None` accent is the palette's own, resolved only by `Flavour::accent`; `theme::wear` is the
  only setter.
  - Every colour is read through its own function (`theme::accent()`, `theme::background()`): no
    pane names a constant; a theme change is one write behind one lock, not a value baked into call
    sites.
  - **Every text-holding measure is read likewise**: `theme::row_height()`, `theme::text_base()` are
    `scaled` off the worn `TextSize`, so one setting moves type ramp, rows, sidebar, header,
    controls. Only compositor-owned values stay `const`: `CORNER_RADIUS`, `RESIZE_BORDER`, window
    marks, rail track and thumb, `WINDOW_MIN_*` (read once at window open).
  - **Ink on an accent is chosen by contrast, not a lightness threshold**: `theme::ink_over` weighs
    the flavour's `pitch` against `paper`, answers the better reader (deep accent: paper; pale:
    pitch), so generated palettes need no hand-picked ink. Takes a colour, not the worn accent, as
    `kit::dot_swatch` draws its check on an accent not yet worn.
  - Semantic colours come off the worn accents, no second list: `bit_perfect`, `lossless`, `done` =
    green; `repacked` blue; `dithered` amber; `failure` red; `suspect` peach; `converted`, `lossy` =
    muted grey. One colour, one meaning (playback bar's signal path, inspector stages, row format
    badge, device badge) in every palette.
  - `theme::tinted` makes a colour a wash (selected chip, playing row, badge ground; no second
    palette of washes); `theme::selection` is the accent worn thin.
- **Every theme is dark; contrast tests hold each readable.** Sixteen: Resonate's own warm
  near-black, Midnight, Graphite, AMOLED, Plum, Rosé Pine, Rosé Pine Moon, Catppuccin Mocha,
  Macchiato, Frappé, Nord, Gruvbox Dark, Tokyo Night, Dracula, One Dark, Solarized Dark. Each
  carries the same seven accents (Mauve, Blue, Teal, Green, Amber, Peach, Red: `Accent::ALL`) under
  its own hex: an accent is part of a palette, not laid over it.
  - **A fresh install wears the palette's own accent.** Each `Flavour` declares a `native` accent
    (Nord's frost blue, Gruvbox's amber, Catppuccin's and Rosé Pine's mauve); `Appearance::DEFAULT`
    names none; the Colour card offers it as a ringed swatch ahead of the seven, so switching
    palette moves the accent until one is picked. Choosing none again = `Setting::Accent(None)`,
    removing the `accent` key from the file (as a cleared contact does).
  - Sixteen fills the shelf: theme cards are `theme_swatch()` wide in a `settings_column()` body,
    four a row (palettes added four at a time keep rows full).
  - **AMOLED's panes are pure black** (`background` and `surface`: pixels behind lists off on OLED);
    raised and hover still step up; wears Resonate's accents.
  - **Where a published palette names no hue for an accent, its terminal mapping decides**: Dracula
    has no blue (ANSI blue is its purple, magenta its pink), so those are its Blue and Mauve; built
    around the purple. One Dark's `abb2bf` reads 6.6:1 on its ground, so body text is highlighted
    `d7dae0`, `abb2bf` muted. Solarized's `base0` reads 4.8:1, so text is `base2`, muted `base1`;
    its violet, orange, red carry ink at 4.5:1 only on pure-black pitch, which its pitch is (no
    lifted accent).
  - **Contrast bars** (`theme.rs` tests walk every theme x every accent and none): text 7:1 on
    `background`, `surface`; 4.5:1 on `raised`, `hover`; `muted` 4.5:1 on `background`, `surface`,
    `raised`; `faint` 4.5:1 on the panes; ink on an accent fill 4.5:1; accent 3:1 against
    `background`; inks step down `text` > `muted` > `faint` on the ground.
  - `faint` is text (count, key, year, placeholder), so it meets the body bar. Where a published
    palette's own step reaches it that step is quoted (Catppuccin's `overlay2` faint, `subtext0`
    muted; Frappé's `subtext1` muted, its `subtext0` 4.3:1 on `surface0`; Gruvbox's `fg4`;
    Solarized's `base0`); elsewhere faint is lifted along its hue until it reads (the ramp's
    `THIRD_INK` included). Nord's aurora red alone could not carry its ink at 4.5:1 and is drawn a
    fifth of the way to `nord6`: the guard exists rather than trusting a famous palette.
- **A palette is quoted or ramped; hand-writing twenty is how one drifts.** Quoted = a published
  palette's own hexes (Mocha stays Catppuccin's). Ramped = `ramp` over a `Recipe` (native accent,
  hue, chroma, alarm, `LitAt`) through a fixed lightness ladder (ground, below-ground, raised,
  hover, hairline, drawn edge, three inks), then all seven accents at the canonical hues. Midnight,
  Graphite, Plum are recipes; Graphite = Midnight at chroma nothing. Flavours are `static
  LazyLock`s; a ramped one cannot be `const` (HSL-to-`u32` is arithmetic a `const` context will not
  run). `hsl` is pinned by a test (grey, black, white, pure red, green, blue:
  `a_ramped_palette_reads_back_the_lightness_it_was_asked_for`) so the ladder cannot shift under its
  palettes. The three share the seven *hues* (an accent is what a colour *means*: green is
  bit-perfect wherever drawn) but each lights them via its own `LitAt`: Midnight pale and soft on
  navy, Graphite greyed towards its ground, Plum deep and rich, so they no longer share a swatch row
  (`each_ramped_palette_lights_the_accents_its_own_way`); contrast tests hold every one.
- **Two faces; figures always in the second.** Inter for text, JetBrains Mono for anything compared
  across rows (clocks, rates, counts, bitrates, kbps, inspector tag values). `theme::mono` builds
  the `Font` with `tnum` on (figures align down a column); `kit::figure` is the one way to draw one.
  Neither face is embedded, so **the drawn family is settled once against the machine**: `fonts.rs`
  holds a ladder per face (Inter, Adwaita Sans, Cantarell, ...; JetBrains Mono, Adwaita Mono, ...);
  `fonts::settle` walks it against `cx.text_system().all_font_names()` at application start and
  answers the installed spelling (no Inter: Adwaita Sans or Cantarell, not whatever fontconfig
  resolves). Neither ladder offers a family the other does (test), so a body face is never taken for
  a mono one; a machine with none keeps the wanted family and lets gpui fall back. Not a setting, no
  key (a key with no control is what the settings rebuild went hunting); the About card reports the
  two (`fonts::drawn_in`). `theme::text_xs()` through `text_title()` and `text_lyric()` are the
  whole type scale, set via `text_size` not gpui's rem steps, so the root's
  `text_size(theme::text_base())` is what a bare `div` inherits.
- **A control is a `kit::button` in one of four tones; nothing draws its own.** `Tone::Primary` =
  accent fill, the one gesture a heading leads with (*Play*). `Tone::Outlined` = raised secondary
  (*Play next*/*Add to queue*, one pair one tone wherever together; *Save as a playlist*; *Show
  graph*). `Tone::Destructive` = raised fill edged and lettered in the failure colour, only for
  losing something for good (the delete dialogue's *Delete*). `Tone::Ghost` = everything else. Each
  has an icon slot and a hint. `kit::icon_button` = the square version (row controls, playlist index
  actions); `kit::chip` = pill an order, reading, cap or setting is chosen from; `kit::badge` =
  small mono tag (codec, playlist kind, device standing). `kit::format_badge` = codec badge beside
  depth and rate from `format::quality` (*24-bit / 96 kHz*, never `24/96`, which reads as a
  fraction), coloured by `Codec::is_lossless`. The row's format cell is `theme::row_format()` wide
  (*32-bit float / 44.1 kHz* beside its badge is the longest it holds).
- **Pressability is the builder's decision; a greyed control carries no press at all.** `kit::Press`
  = `Takes` | `Greyed`; `kit::button_when` and `kit::mark_when` read it; `kit::button` and
  `kit::icon_button` are those under `Press::Takes` (thirty-odd call sites unchanged). `Greyed`
  draws at `kit::GREYED`, takes `cursor_default`, no hover style, pointer cursor or
  `icons::lit_on_hover`: nothing under the pointer says it would answer. Must be decided where the
  element is built: gpui's `hover` has `debug_assert!(hover_style.is_none())`, so a second call
  panics a debug build and hover cannot be painted on then off. `settings::action` spends it: on
  `busy` it returns the bare `Press::Greyed` button, else attaches listener and focus ring
  (`in_the_ring`), so `action` (*Add folder…*, *Rescan folders*, *Stop*) never hangs a press on a
  greyed control. The folder row's forget mark is always `Press::Takes` (it waits, below). *Rescan
  folders* is greyed by `LibraryModel::is_busy`, which `start_scan` also refuses on.
  - **Adding and forgetting a folder are not greyed: they wait.** `LibraryModel::add_roots` and
    `forget_root` put the request in `roots_waiting` (`RootWaiting::ToAdd`/`ToForget`) wherever a
    `Work` runs; the Folders group says what waits under its rows; `take_up_what_waited` runs
    wherever a pass hands its `Work` back: a forget first, one at a time, then every waiting folder
    in one scan. Before, a scan greyed both, so the second of two folders could not be added until
    the first was read. A forget mark is keyed by its root (`ElementId::Path` under a `"forget"`
    child), not one id every row shared.
  - **An enrichment is deliberately not a `Work`**: `LibraryModel::enriching` is a slot of its own,
    started with `for_the_pass` (reference and fingerprinters that yield to the window's own
    requests, `online.md`), never the model's `reference`. A running lookup greys the Online card's
    *Look up*, *Refresh all* and the Library card's *Enrich* via `held_back`; a scan can still start
    under one. Visible from every pane: `RootView::enrichment_status` sits above the sidebar's
    *Settings* row only while `LibraryModel::is_enriching`: a globe in `theme::faint()` and
    *Enriching…* alone (no counts, the listener asked them off; ellipsis where the sidebar is
    narrower); a press opens `Category::Online`. The Online card's *Look up* group says *Looking
    up…* on its button and nothing more while running; what it found is the toast `looked_up` tells
    as it joins.
- **What a listener reaches for is what the lookup asks next.** `LibraryModel` holds one `Sought`
  for the run and hands it to every `enrich`, so a nudge made while idle is there when a run starts.
  All via `LibraryModel::ask_about`: scoping the tracks pane to an album (album plus owner,
  `album_anywhere`'s `artist_id`); to an artist; playing a row (the started track's `album_id`,
  `artist_id`, from `RootView`). Also, whenever a listing is read with an artist scoped,
  `ask_about_what_is_drawn` names the drawn albums (last first) and the artist. Only reorders what
  the pass has left (an album asked yesterday is not asked again by a click): a nudge, not a
  command; `library.md` has how the pass reads it.
- **Every pane opens with the same heading; a page inside a category opens with a hero.**
  `kit::heading` holds a `kit::heading_row`: eyebrow naming the section (*LIBRARY*, *COLLECTION*,
  *NOW PLAYING*, *SETTINGS*), `kit::title`, `kit::subtitle` (counts, total length), `kit::actions`
  on the right; under it the *Reads* chips (`listing::reads`) and a naming row where a pane has one.
  - **The name keeps its room; actions drop under it.** Each name block holds
    `theme::heading_name()` at least and `kit::heading_row` wraps, so where least-name and actions
    do not fit side by side the actions take their own line (up to the whole row, wrapping inside):
    a long playlist name is not ground to a letter by thirteen controls; no action is pushed off.
    Before, actions wrapped inside 64 % of the row beside a name free to shrink to nothing: at the
    720 px window the queue's *Save as a playlist* ran off the edge, and the settings pane's 400 px
    find field, wider than 64 %, spilled left over the subtitle.
  - **An album or artist is not a pane heading with a picture bolted on**: `album_page_heading` and
    `artist_page_heading` = `kit::way_back` top left and a `kit::hero` under it. Album cover,
    portrait, opened suggestion's art: each a square of the text column's own height, decoded at
    `Drawn::OnThePage` (sharp at scale 2), so the picture meets the text with no empty band. Column
    = eyebrow, `kit::hero_title` at `text_title`, lines about it, bottom-aligned to the picture.
    Height = the column's, read a frame behind by an absolute canvas filling it
    (`kit::measures_its_height` into `RootView::hero_height`, via `hero_side`), so the inset
    resolves against the height the text already took (a zero-width sibling stretched beside the
    text came out a line taller than the column and left a band above the eyebrow). Until measured
    the cover uses `scope_cover`.
  - Action row = the column's last child, under the text, clear of the cover: Play, Shuffle (album,
    artist), favourite, a sort icon where the listing sorts. Shuffle reads the same scoped listing
    as Play, begins at a time-chosen row, enables queue shuffle via `RootView::play_shuffled` (the
    suggestions page too). **Play beside it plays in order**: every whole-list Play (tracks, album
    and artist headings, an opened playlist, a playlist's index row and card, a suggestion) sends
    `SetShuffle(false)` via `RootView::plays_in_order` before loading, so shuffle left on does not
    scramble the album pressed Play on.
  - A row pressed to play from leaves shuffle as is and queues what Play would:
    `RootView::play_the_listing_from` plays held rows where they are the whole listing
    (`is_the_whole_listing`, against `listed()`), else reads it whole via `with_everything_listed`
    and starts at the pressed track (a press or Enter on row 1 990 of a 20 000-track library once
    queued only the pages read so far:
    `a_row_played_from_a_listing_read_in_part_queues_the_whole_listing_from_that_row`).
  - An album adds the info mark (and *Get the rest* where tracks are missing); an artist adds one
    where it has genres, and where it holds albums the Albums/Tracks choice sits at the row's right
    (listing follows the title, no band between); unheld releases stay on the row. Play next, Add to
    queue, Add to playlist are not on the page. The row does not wrap (a wrapping row measured to
    the column laid each button on its own line: taffy sizes a wrapping flex item at the column's
    min-content width). The title has the whole column and wraps, not clips.
  - **The hero's column is measured; what wraps in it must be told its width.**
    `kit::measures_its_width` = a canvas writing `RootView::hero_width` a frame behind (same shape
    as the album grid's `kit::measures_the_grid`, a builder of its own writing a `GridWidth`); title
    and services line take it through `kit::hero_title` and `kit::wraps_within`. Until the first
    frame measures, the title is one line ending in an ellipsis.
  - The opened playlist uses the same hero, cover = `views/mosaic.rs` art (suggestion bullet below).
    Beside the cover: text; a faint line saying when it was made and last played; left-aligned Play
    and Shuffle; then *Add songs*, *Sort*, *Tidy* for a list; the pin as a mark lit while pinned;
    `Icon::More`, opening the index card's right-press menu (queueing, pin, rename, *Duplicate*,
    export, discard) at the press, so the page does everything the index can. The title opens the
    shared rename field with a rename selector revealed on hover; no separate rename button.
    `kit::way_back` reads *Playlists*, above eyebrow *PLAYLIST* or *SAVED SEARCH*.
  - **A title that is a link is `kit::linked_title`, never `kit::title` over an `opens`.** Lyrics,
    inspector, visualiser, analysis headings name the playing track via `opens`; under
    `kit::title`'s block box the link got no width and the heading drew its ellipsis alone.
    `linked_title` = same face as a flex row holding the link `keeps_its_width` (the by-line's
    artist's shape). **Such a link is cut before drawing**, gpui never truncating no-wrap text past
    its first measure: `RootView::playing_heading` (eyebrow, title, artist shared by lyrics,
    visualiser, analysis headings) measures itself into `heading_room` a frame behind and cuts both
    names there with `kit::cut_to_fit`; the inspector cuts its title at `inspected_room` (uncut, a
    long title was sliced by the pane's edge with no ellipsis). `by_line` cuts the artist at its
    room too, the album taking the rest, so an artist longer than the panel ends in one. The artist
    under it is `kit::linked_subtitle` for the same reason plus one: `kit::subtitle` over an `opens`
    stretched the link across the column, so a press well right of the name still opened the artist.
- **A track row is the same eight cells wherever drawn.** `listing::columns` = the header the tracks
  pane, queue and opened playlist put over their rows; `number_cell`, `title_cell`, `artist_cell`,
  `format_cell`, `heard`, `length_cell` the cells under it: no two disagree about a width. The
  header fills a row's width, so its growing title cell leaves artist and later columns where the
  row puts them; its trailing space matches the controls rows draw (an oversized blank cell squeezed
  the title and shifted later columns left). Title and artist give way together: `title_room` and
  `artist_room` both start from `theme::row_artist()` (200 px) and both shrink, only the title
  growing (a `flex_1`-from-nothing title beside a fixed artist was the first thing a narrow window
  took, to the last letter, while the artist kept all 200 px). A track with other copies carries
  `+N` *inside* the title cell (`tagged_title_cell`, title `flex_1` beside it): the tag takes room
  from the title, not later cells; both text cells end in an ellipsis, not a square cut. The playing
  row draws `listing::playing_mark` in the number cell and its title in the accent. Row controls sit
  in `browser::row_controls`, invisible until hovered. Track rows keep Play next and Add to queue;
  the tracks heading has Play and Shuffle for the whole listing, no Add to playlist.
  - **A listing narrower than its columns gives up HEARD, then FORMAT, before the names.**
    `listing::Shown::within` weighs the width the header measured into `RootView::columns_fit` (a
    `listing::Fitting`) against the fixed cells, `NAMES_AT_LEAST` (title and artist together) and
    the controls that listing draws; `columns` publishes the choice and every row reads it back
    (header and rows cannot disagree); rows use the `COLUMN_GAP` and `ROW_INSET` the arithmetic
    counts. An unmeasured header draws every column. Before, fixed cells with no room ran past the
    pane's edge (LENGTH clipped to *LENGT*, FORMAT over the rows' titles) while title and artist
    were ground to nothing
    (`a_narrowing_listing_gives_up_what_was_heard_then_the_format_before_the_names`).
- **Whatever a gesture takes out of the queue is kept; a toast says so.** A playlist edit answers
  with a `Notice` and an undo; the queue answered with rows simply gone. `TakenOut` = what the last
  gesture took (rows, the row they started at, the id of the row before them); `RootView::took_out`
  is what the offer stands on. `drop_rows` writes it, so a row's ✕, a reach of thirty rows and
  backspace all keep what they take; *Clear* is the same call over `Span::between(0, last)`, not a
  `Command::Remove` of its own. `TakenBack::keeping` answers how many rows it kept; `drop_rows`
  tells it as a toast (*Took 34 tracks out of the queue*), not a line under the heading standing as
  long as the offer; *Put back* stays in the heading and sends the rows as a `Command::Insert` at
  `Placement::At` the row they came out of. Kept as `QueueItem`s, not ids (`one_id_each` mints new
  ones on the way back in).
- **Dropping rows leaves the reach on a row still there.** `drop_rows` weighs the rows held before;
  `Reach::after_dropping` lands on the first row after the gap (what moved up into it), or the row
  now last where the drop took the end, nothing where it took every row. The reach used to stay on
  the dropped span's first row: past the end of a shorter list it drew no mark while `reach_in`
  pulled the next `delete` back onto the last row, never highlighted.
- **A run of gestures is walked back one at a time; nothing queued since takes the walk away.**
  `TakenBack` = the bounded stack `TakenOut` sits in: *Put back* pops the top and sends its rows as
  a `Command::Insert` at the row after the one they followed (`TakenOut::after`, the id of the row
  before them when taken), which `landing_in` finds wherever it now stands: the front where they
  were the front, where they came out where that row is gone too. A step stands while none of its
  rows is back in the queue (`stands_over` weighs the queue against a set of their ids, which
  `Unclaimed::claim` hands back to
  a row on its way in): an album queued, a row dragged or a track played between a *Clear* and a
  *Put back* leaves the offer standing, and a row already queued again is not put back twice.
  `KEPT_GESTURES` bounds the walk at sixteen, oldest out first; the button's hint says how many are
  behind.
- **What was put back can be taken out again, walked forward as it was walked back.** *Put back*
  moves the popped step onto a second stack; *Take out again* pops that: `TakenOut::standing_in`
  finds its rows by id as the run they were put back as, wherever edits moved it, and sends
  `Command::Remove` over it, restacking the step so *Put back* offers it again. A step whose rows no
  longer stand together is not offered; a fresh gesture clears the forward walk (as a new edit
  clears any redo). Neither `drop_rows` nor a toast (the gesture already told about, made again).
  **The walk has the undo keys while the queue is in front**: `RootView::undo_edit`/`redo_edit` try
  `put_the_queue_back`/`take_the_queue_out_again` first where the pane is `Pane::Queue`, reaching
  the playlists' undo only where the queue has nothing to walk.
- **The offer is drawn while a step stands.** An emptied queue is the one place the offer could be
  seen, so the heading is drawn over the empty pane while a *Put back* step stands, *Clear* and
  *Save as a playlist* hidden (nothing to act on).
- **A card leaves out what the file does not declare; cards stack in two columns.** `fields` takes
  an `Option` per row and draws only what is there, one faint line where a whole card is empty
  (shared by all five: a file with no ReplayGain no longer renders five rows of *unknown*). The five
  were `flex_1` in a `flex_wrap`, stretching every card in a row to the tallest and leaving the last
  row ragged; `beside` deals them into two `flex_col` columns that stack tightly and wrap to one in
  a narrow window.
- **The inspector is the one pane reading the catalog, not the engine; the *Heard* card is why.**
  Every other card is the `StreamDigest` or `OutputStatus` (decoder and sink on the playing track);
  play count and last play are the catalog's, said nowhere else on screen. The reading comes off the
  `Track` `Playing` already resolves through `LibraryModel::track_of`: no query. `Heard` = three
  fields on `Playing` (`heard_card`); a row no scan has seen answers *no library row*, not zero
  (a different thing). `format::since` reads dates as the largest span that fits (*3 days ago*), not
  a stamp.
- **What a search matched is lit where drawn, by one element.** `listing::matched` answers the text
  itself where nothing is lit, else a `StyledText` with `HighlightStyle` runs: an unsearched row
  costs no more; a lit one still truncates, wraps, measures as one string. Runs are `Search::lit`
  for that cell's `Column` (`library.md`), so a name reached by a fold lights the spelling drawn
  (typing *przybylowicz* lights *Przybyłowicz*). Only browse panes call it (the only listings a
  search narrows): track row title and artist, album cell title and artist, artist row name; queue
  and opened playlist pass no runs. A run is the accent (the playing title's colour) at
  `FontWeight::MEDIUM`; the playing row's title cell is itself accent and MEDIUM.
- **The albums pane is a grid whose column count follows the window in the frame it moves.** A cell
  is `theme::grid_cover()` square, title and artist-and-year under; a `uniform_list` row is
  `columns` of them (two thousand albums cost a screen of cells a frame). Columns read
  `RootView::grid_width`, a `kit::GridWidth`: the canvas under the list (`kit::measures_the_grid`)
  writes on prepaint how much *narrower than the window* the grid is, and `RootView::region` tells
  it the window's width whenever the pane is built, so it answers window width now less that
  (sidebar and paddings fixed: a resize is the same pixels on both). When the narrower-by moves the
  canvas asks for the next animation frame (a refresh asked mid-draw is ignored).
  - **The list is not built until that cell holds something**, as the lyrics pane's `place` reveals
    nothing until the measured size is the size it has: the `max(1)` fallback is a column count
    nobody wants, and full-width cells stacked one a row is the pane's most visible failure. The
    canvas is `absolute` and sized by the container, not the list, so it measures the same bounds
    with the list there or not: holding it back is safe.
  - A resize draws at the new count the first frame it is drawn
    (`a_grid_is_as_wide_as_the_window_now_less_what_it_was_narrower_by`); only a change the window's
    width does not carry (a sidebar grown with the text size) takes the measure's frame. A cell's
    cover is `Drawn::InAGrid`, one of four `Drawn` sizes (`InARow`, `NowPlaying`, `InAGrid`,
    `OnThePage`), sized by `Scale::texels_for`.
- **An empty pane says what is missing and what to do.** `kit::empty` = the pane's icon in a ring, a
  sentence naming the state, and where there is one a second naming the gesture that fills it ("No
  albums yet", then where to add a folder; an empty queue, then where a row comes from).
- **A search that matched nothing offers the spelling the catalog holds, as a press.**
  `kit::empty_offering` = `kit::empty` plus one child, what the three browse panes'
  `RootView::nothing_matched` hands it: a `Tone::Outlined` button *Did you mean Billie Eilish?*
  putting the text in the search field via `RootView::search_instead` (the field's own observer
  takes over), so the correction lands in the box, editable, not run behind the listener's back. It
  draws `LibraryModel::instead`, which `browsed` fills only where albums, artists, tracks and sung
  matches all came back empty (it cannot contradict a pane listing something); with no spelling but
  the words sung somewhere, `nothing_matched` offers *Sung in N tracks* via `sung_offer`.
  `library.md` has how the spelling is found. All three panes ask through one method, all emptying
  on the same search; albums and artists panes build it before the heading (the model's borrow must
  end before `cx.listener` takes one).
## Drawing

- **A plot is quads, never a path.** gpui 0.2.2 rasterises each vector-path batch through a
  window-sized 4× MSAA texture cleared and resolved every frame: costlier on an integrated GPU than
  the plots (scope, inspector bitrate graph, analysis waveform/spectrum, equaliser curve).
  `views/plot.rs` uses batched `paint_quad`: `stroke` = quad over each segment's bounding box (rise
  plus line width tall, never narrower than the line); `wash` = quad from each segment's middle
  height to a level (floor for bitrate/spectrum; zero line for the equaliser's signed curve, so a
  cut washes up to it); `between` = quad per column from the higher of two traces to the lower
  (waveform envelope). Gradients lie over each quad, not the plot, so run from the trace down;
  points a pixel or three apart make steps read as the line. `PathBuilder` has no caller in the
  crate.
- **An icon is an embedded SVG reached by an `Icon` variant, never a text glyph; one list is all an
  icon is.** `icons!` takes variant and file name, writes the enum, `Icon::ALL`, `Icon::path`,
  `Icon::drawing`, so a variant cannot be missing from the array the `AssetSource` walks (once it
  was: compiled, answered `Ok(None)`, rendered nothing, uncaught as `path`/`drawing` were exhaustive
  and the test iterated `ALL`). `crates/resonate-ui/assets/icons` = one silhouette per variant
  (`include_bytes!`); `icons::Embedded` is the `AssetSource` given the `Application`: an
  unanswerable path cannot be written, the error enum needs no missing-asset variant.
  `Icon::Resonate` = the app's mark, the one icon with no file there: the header wordmark fills the
  accent square with it, *drawn out of* `packaging/resonate.svg` (five paths under
  `translate`-and-`scale`, over the rounded ground, in a gradient an alpha mask cannot hold).
  `icons::masked` takes that file's `<g>` alone (strokes, width, caps), drops its `transform`, swaps
  gradient for plain stroke, sets a 24-unit `viewBox`, once, behind a `LazyLock`: the launcher icon
  is the only place the mark is written. `icons.rs`'s test holds the mask to the packaged paths,
  refuses ground/gradient/placing riding along, and reads `Icon=` from the desktop entry to hold the
  asset name to it (`app.rs` does likewise against `APP_ID`). gpui renders an SVG to an alpha mask
  tinted by the element's `text_color`: icons carry and inherit no colour, so each gets one at the
  call site; `icons::lit_on_hover` follows its button's hover through a group. Every icon is a
  silhouette on a 24-unit box; a two-tone mark (badge over icon) = two overlaid `svg`s. No control
  uses a glyph: window controls are `Window*` icons; search clear mark, a row's ✕ and the queue
  row's arrows are `Icon::Close`, `ChevronUp`, `ChevronDown` (a box keeps its mark centred where a
  glyph's baseline did not). **A control's box and mark are one declaration:** `theme::Control::of`
  takes hit area and inner mark and asserts in a `const` that the mark leaves `MARK_MARGIN_AT_LEAST`
  on every side, so a box shrunk under its mark or a mark grown past its box fails to build (else a
  glyph clipped at the button edge). Measures stay functions (`row_control`, `row_control_icon` read
  one `Control`'s halves); a control borrowing another's mark gets its own: search clear `CLEAR`
  (once an unscaled literal), settings filter `FILTER_CLEAR`, accent swatch tick `CHECK_MARK` (not
  the hint's size).
- **The launcher's icon wears the accent; the binary puts it there.** `launcher::AppIcon::of` is the
  whole decision: worn accent = `Appearance::DEFAULT`'s → `Packaged`, else `Recoloured`
  (`packaging/resonate.svg`'s two gradient stops → accent lifted towards white and accent; root
  `id="resonate-in-the-accent"`): a palette switch moves the icon with the accent until one is
  chosen; returning to Resonate's own removes it. `AppIcon::drawn_here` reads that id (file ours to
  replace or remove); an icon under that name nobody here drew is left alone **unless it is a
  byte-for-byte copy of the packaged icon** (a hand install of `packaging/` or an unpackaged tree
  run leaves one). That takes the accent like an empty place, but what is written opens with
  `data-written-over="the-packaged-icon"` beside the id, so returning to Resonate's accent writes
  the packaged copy back, not leaving the launcher iconless
  (`a_copy_of_the_packaged_icon_takes_the_accent_and_is_put_back_after_it`, binary's `launcher.rs`).
  `run` shows the opening appearance, `RootView::dress` every later one, via `Launcher`, a seam on
  `Stored` like `Present` (no I/O in `resonate-ui`). Behind it the binary's `launcher::Icons`: a
  thread waiting `SETTLES_AFTER` of quiet (a run of presses placed once) writes a staged copy over
  `$XDG_DATA_HOME/icons/hicolor/scalable/apps/resonate.svg` or removes it; only where that moved
  anything, flushes what the install scriptlet flushes (theme folder mtime, `kbuildsycoca6`,
  KIconLoader's `iconChanged` via `resonate_mpris::tell_the_icons_changed`) plus, where the folder
  holds an `icon-theme.cache`, the GTK cache via `gtk-update-icon-cache` or its GTK 4 twin (a stale
  cache hides the new file from every GTK desktop). Opening in the appearance the file already shows
  writes and flushes nothing. **The gradient is `userSpaceOnUse`** over the mark's 24-box: under
  default `objectBoundingBox` each stroke is a zero-width box, which the SVG spec says paints
  nothing; librsvg drew an empty tile.
- **The catalog is read while gpui starts, so the first frame holds it.** `run` starts `FirstRead`
  before `Application::new`: a thread reads what a model with nothing chosen asks for
  (`Asked::at_first`, also what `LibraryModel::new` is built from, so they cannot drift) while gpui
  spends forty-odd ms of the main thread on system fonts and Vulkan. `LibraryModel::new` takes it
  from `ResonateApp`; the first read takes it synchronously where landed, awaits it where not, loads
  afresh where the model now asks something else. 616-track catalog: first frame with albums 105 ms
  after `main` (was 112–122), none drawn empty first; the rest is gpui's font scan and device
  creation.
- **`resonate-codec` decodes and scales cover art; gpui gets pixels.** `Drawing::no_larger_than` and
  `Drawing::squared` answer a `Raster` (width, height, pixels blue-first as gpui's atlas holds
  them); `models::picture_of` wraps it in a `RenderImage`: decoded once, by us, never re-encoded as
  PNG for gpui to decode again. A picture over `LARGEST_COVER_SIDE` (8 192) pixels either way is
  refused before decoding; a shrink weighs each pixel by alpha (premultiplied linear light, divided
  back out), so transparency tints nothing
  (`what_is_transparent_does_not_tint_what_is_shrunk_with_it`). A tag-catalog picture arrives alike
  via `CoverArt` re-exported by `resonate-engine`. Each is read once, held in a `Recent` keyed by
  album, artist or file *and drawn side* (a fresh `Arc<RenderImage>` per frame = a new texture per
  frame). Read and decode run on `Drawer`'s two threads, not the background executor: a miss puts
  the key in the model's `decoding` set and hands over the work, the cell draws a placeholder disc
  until it lands, the landing notifies the model; unread covers cost the render thread nothing, and
  a picture a file lacks is cached as a miss, not asked per frame. A miss holds only until the
  catalog next changes: `LibraryModel::reload` (end of every scan and lookup) forgets every cached
  `None` cover and portrait (`Recent::forget_where`), so a fetched or rescanned picture draws; drawn
  pictures stay (`a_cover_found_after_it_was_first_drawn_is_drawn_once_the_catalog_reloads`). **A
  file's picture is cached only once the engine settled it:** `Player::art` answers nothing on the
  ask queueing the read, so `PlayerModel::art` reads `Player::art_read` (`NotYet`/`Answered`/
  `Nothing`, `TagsRead`'s shape); `NotYet` goes in `unsettled` against the `media_revision` asked
  at, not `pictures`, asked again once it moves (caching that first `None` left the playback bar's
  cover empty for any file the catalog does not cover, even one with its own picture).
  `LibraryModel::warm_the_covers` fills the album cache ahead of the grid once a run:
  `COVERS_WARMED` (256) albums declaring a picture, at the grid's side, decoded one at a time in its
  own task (never more than one decode queued: an on-screen cell waits behind at most one), skipping
  what cache or in-flight set holds, bounded under `COVERS_HELD` on purpose (warming more than the
  cache keeps evicts its own work). `DECODES_AT_ONCE` (4) bounds in-flight decodes in the album
  cache and the per-file one: a cell asking while full is refused *without* being marked decoding,
  so it asks again next frame, and a landing notifies, guaranteeing that frame (else a fling queued
  one decode per newly visible cell on an unbounded FIFO, every core busy on scrolled-past covers
  ahead of those on screen).
- **A cover decodes on one of two threads of its own (glibc gives every allocating thread an
  arena).** A 1280-pixel progressive JPEG costs ~15 MB while decoding; once glibc's dynamic mmap
  threshold has risen, such a block is no longer mmap'd and stays in the arena of the thread that
  freed it. Across gpui's sixteen workers: ~110 MB held after the start-up warm (window 265 MB); on
  `Drawer`'s `DRAWING_THREADS` (2): settles at 158, peak 280 → 158. Two threads keep up with the
  four-wide executor: ~14 ms a cover, not 27. `Drawer` is a gpui `Global`: `draw` queues a closure,
  answers a future (`futures-channel` oneshot) the model awaits; a queue nobody reads (threads
  unable to start) runs the closure where asked. Allocators lost: mimalloc (v3, v2) and jemalloc
  settled higher than glibc (575, 470, 345 MB; a worker that decodes then idles never returns its
  thread cache).
- **What a cache lets go of, gpui lets go of.** A picture is a `RenderImage` the cache owns:
  `Recent::insert` returns what it pushed out or overwrote as a `Leaving`; `Forget` turns it into
  `App::drop_image`, removing its tiles from every window's sprite atlas (album covers, portraits,
  per-file pictures, the magnified picture a new one replaces, the spectrogram a new painting
  replaces), so GPU memory is bounded by the caches (a cover drawn once used to stay in the atlas
  for the run). A forgotten picture still on screen is only decoded again. The one picture still
  handed to gpui as encoded bytes is the Listen sheet's, forgotten by `Image::remove_asset`.
- **A cover is drawn at twice the device pixels it is shown at, at that side alone.** gpui's atlas
  sampler is bilinear without mipmaps, reading four texels however far it reduces (a 1280-pixel
  cover in a 22-pixel cell was point-sampled and crawled). `Drawn` names the cell (`InARow`,
  `NowPlaying`, `InAGrid`, `OnThePage`); `Drawn::side` asks the window's `Scale`
  (`Scale::texels_for`): twice the cell's device pixels below scale factor two (the sampler's four
  texels = exactly the 2×2 to average at 1, 1.25, 1.5), device pixels themselves from two on (1:1).
  `RootView` hands the scale to both models whenever the window's moves; keyed by drawn side, a
  window dragged to another display redraws covers at the new size, not stretched. Nothing is drawn
  at an unasked size: decoded for the one cell wanting it (rows never pay for grid covers; cost: a
  second decode where one album shows in two cells at once). Versus a true area average over this
  library's covers, twice the device pixels is ~sixteen times closer than handing the sampler the
  whole cover; a 128-pixel thumbnail was only a third closer. Scaling is `CatmullRom` in linear
  light, not on sRGB values; a cover already smaller than the cell is handed over as it came.
- **The resize is `image::imageops::resize`'s arithmetic walked in memory order.** The crate's
  vertical pass fixes a column and walks down it (a strided read per tap per pixel, each tap
  converted to linear light: two thirds of a cover's decode). `artwork::drawn_smaller` computes the
  same weights with the same expressions, summing each output pixel's taps in the same order, so
  the result is the crate's to the bit (`a_cover_is_drawn_exactly_as_the_image_crate_would_draw_it`
  holds it to `resize` over random RGBA), but adds a whole source row into a column of sums at once,
  converts each source row to linear light once into `Held` (a ring as deep as the widest tap
  window), and runs the horizontal pass straight from that column, no intermediate image. A JPEG
  stays the `RgbImage` it decoded to (not copied into RGBA); `squared`'s crop is a `Plane` over the
  same samples. The transfer function is a 256-entry table, not a `powf` per channel per pixel
  (exact: 8-bit input); the 8-bit hold costs a 16-bit PNG's intermediate precision, quantised on the
  way in, invisible in a thumbnail written back as 8-bit.
- **The magnifier reads its picture when it opens, not kept beside every thumbnail** (source bytes
  beside each cached picture were a per-entry copy of the original, for a view showing one at a
  time). `LibraryModel::whole_cover` and `PlayerModel::whole_art` read on the way in, drawn no
  larger than `MAGNIFIED_AT_MOST` (4 096) on a `Drawer` thread, keeping one `Magnifying`: the key
  being read, then the key with what it read, so a key answering nothing is not asked every frame
  (`magnified_art` is read each frame the magnifier is open); a read landing after another picture
  was asked for is dropped from the atlas. It draws scrim and title, no picture until the read
  lands; nothing else in the window pays. Nothing magnifies a portrait. With covers no longer
  holding their source, a window on a 435-track library of 1280-pixel covers fell from 689 MiB to
  264.
- **A portrait is a picture cut square, held apart from covers.** `drawn_square` draws through
  `Drawing::squared` (cover: `no_larger_than`): the centred square of the shorter side, scaled down
  to the side asked, never grown, so the sampler gets the square the round frame shows.
  `LibraryModel::portrait` takes a `Portrayed` (`InARow`, `InAGrid`, `OnThePage`; its `Drawn` sets
  the side; no now-playing size), reading `portraits`: a
  `Recent<AtSide<ArtistId>, Option<Picture>>` under `PORTRAITS_HELD` (256) beside the 512 covers,
  drawn on `Drawer` under the same `DECODES_AT_ONCE` bound with `decoding_portraits` in flight, only
  where `Artist::has_portrait` says there are bytes. `browser::portrait_frame` = the round frame
  (`ObjectFit::Cover` in a `rounded` div) at `theme::avatar()` in the artists list and
  `theme::scope_cover()` in the scoped heading; `kit::avatar` draws where there is none.
- **The magnifier's scrim occludes, so the press closing it closes only it.** An `absolute` child
  over the whole app whose hitbox lacked `occlude` was one more hitbox, not a wall: the press
  shrinking the cover also hit the row, cell or control under the pointer (closing a cover over the
  albums grid opened the album beneath). The menu's scrim already occluded; the magnifier now does.
- **A cover is drawn wherever a row names a track, from one cell.** `RootView::cover` takes a
  `Pictured` (an album, or a track: `Option<AlbumId>` beside its file), so albums, tracks, queue and
  playlists panes draw the same 22-unit cell. Album art preferred, `Player::art` the fallback (a
  picture on a row no scan has seen; a file with none loses only the picture).
  `RootView::drawn_cover` answers the art and a `Magnified` naming its source, so the clickable
  transport cover opens whichever it drew. The overlay over the whole app is sized from
  `Window::viewport_size`, not a constant: the shorter window edge, capped
  (`theme::magnified_cover`), never filling a wide screen; nothing pages from one cover to the next.
- **The window holds decoded pictures apart from their bytes.** 256 per-file pictures
  (`PlayerModel`, `PICTURES_HELD`) beside 512 album covers (`LibraryModel`, `COVERS_HELD`), in
  `Recent`s bounded by count (the engine's `ROWS_HELD` and `ART_BYTES_HELD` bound bytes): neither
  window cache is bounded by weight. A `Recent` keeps a `BTreeMap` from use clock to key beside the
  entries, re-keyed on each look, so eviction is `pop_first`, not a minimum scan (the engine's
  `Held` shape; a scroll past the 4 096-row `ROWS_HELD` bound walks no four thousand entries per
  newly visible row). It evicts the entry nothing looked at, never emptying itself, so a row
  scrolled well past either bound is read again on return (up to two SQLite reads the first time a
  queue row is seen: by id, then path). `LibraryModel::named` is emptied by `reload` (with
  `read_albums`), by no plain `read`; `track_heard` writes the row `Library::track_played` answered
  into `named` before reloading, as `remember` does for scanned tracks.
- **Text answering the pointer is lit when the element is built; gpui cannot light it later.** gpui
  0.2.2 resolves a `hover` style only in *paint* (`compute_style_internal(None, …)` in
  `request_layout` and `prepaint`), while a text child is shaped in `request_layout` with the
  moment's colour baked into its runs: a `hover` changing a background works; one changing
  `text_color` or adding an underline never reaches a glyph (every link `opens` drew, column
  headers, a ghost button's label, a segment's promised a hover nothing painted).
  `views/pointed.rs`: an element following the pointer carries an `on_hover` writing its `ElementId`
  into `POINTED_AT` (thread-local) and asking for a redraw; the next build reads `is_pointed_at` and
  shapes the text lit, as a selected row's already is. `LitUnderThePointer::lit_under_the_pointer`
  is the whole gesture (an id and what to do to the element while pointed at); `follows_the_pointer`
  its half for an element lighting something inside it (an album cell lighting its title). A set,
  not one slot, as a cell holds a link: the pointer over the artist under a cover is over both.
  `set_pane` and `show_everything` empty it (a link pressed to go somewhere is removed while pointed
  at, never hearing the pointer leave); the pointer leaving the window empties it via
  `pointer_watch`. A control with a background keeps its `hover` for that, only text going through
  `pointed`.
- **A hint names its key one way, and the key is the one bound.** `keys.rs` is `key!`, the spelling
  of every key a hint names (`key!(play_pause)` = `space`, `key!(raise_row)` = `alt-up`);
  `app::bindings` binds through the same `key!`, so a hint cannot name an unanswered key. `keyed!`
  writes the one wording: what the key does, an em dash, the ways (last two joined by *or*, earlier
  by commas): `Louder — the wheel or ctrl-up`; a hint saying more = such sentences joined by full
  stops, a described state first: `Repeat is off. Repeat the queue — r`. Both are `concat!` all the
  way down, so hints stay `&'static str`
  (`every_key_a_hint_names_is_a_key_something_is_bound_to` walks the bindings for each).
- **A control names itself through one seam, and the seam lets go.** `hint::Names` is the only
  place a tooltip is attached (a test walks the crate's sources to keep it so). It is gpui's plain
  `tooltip`, never `hoverable_tooltip` (`clear_active_tooltip_if_not_hoverable` is a no-op for the
  hoverable one: neither press nor scroll takes it down, and every hint here sits on something
  pressed or in a scrolled list). `hint::asking` is the other half: one atomic written once a frame
  by `RootView::render` (`hints_are_wanted`); where it says no, no builder is attached, which gpui
  reads as an instruction to drop a live tooltip on the next prepaint. No while a slider is
  grabbed, an equaliser band held, the picker (`adding`), a magnified cover, a record or a deletion
  prompt (`deleting`) is up, a row is dragged, the pointer is outside the window, and for the one
  frame in which what lies under an unmoved pointer did move: `RootView::laid_out` stamps pane,
  settings category, queue revision, the three listings by `Arc`, the playlist count and the landing
  list's scroll offset; `UnderThePointer::moved_since` weighs stamp and pointer position against
  the frame before, so a row removed from under a still pointer, a pane stepped to by keyboard or a
  list scrolled by a key takes down the hint naming what was there (gpui hit-tests only on a mouse
  event, leaving it over the control that took its place). Reported defect: gpui's `MouseExited` arm
  is the one not updating `Window::mouse_position`, so a pointer leaving the window leaves the hint
  believing it is hovered, and the 16 ms poll draws it there for the run. `RootView::pointer_watch`
  = a zero-size canvas registering `MouseExitEvent` and `MouseMoveEvent` (and `MouseDownEvent`,
  stopping typing) window listeners from its *paint* (`field.rs`'s `follow_the_pointer` idiom; the
  only phase `Window::on_mouse_event` may be called in); its flag also closes the lyrics pane's
  hover, so a pane opened by the pointer does not stay open once it has gone.
- **Every control names itself in a tooltip, and a tooltip wants a pointer.** Playback bar, sidebar
  rows, window controls, queue row arrows and cross, every action in the tracks and playlists
  headings, each row's queue marks, the search clear mark, the info mark holding the search grammar
  and a folder's forget mark all carry one; a toggle names its state and the key changing it. A list
  row says what it is only through its drawn text, so a keyboard-driven session sees none.
- **A playlist row no scan has seen draws as a queued one does.** `views/listing.rs` owns the `Row`
  the queue and playlists panes build: the scanned `Track` where the library has one,
  `Player::media` where not, the file stem only where the file cannot be read. An imported playlist
  of unscanned files reads as titles and artists, not stems, with no new dependency (the tag catalog
  already feeds the queue pane). The play count rides on the same `Row`, drawn by `listing::heard`:
  one `theme::row_plays()`-wide cell for the tracks pane, both list panes and the playlists index,
  so none disagree on width or words. A row no scan has seen carries a count of nothing and draws an
  empty cell (what the catalog says about a file it never met).
- **The cell says how often and how lately, the catalog being ordered by both.** `played:` narrows
  and `SortOrder::Played` orders, with nothing on screen to read against (last-heard was drawn for
  the playing track alone, in the inspector's *Heard* card). `how_often_and_how_lately` folds the
  date into the count's cell (no eighth column on rows of seven): *12 plays · 3d*; the header reads
  HEARD, not PLAYS (the word the inspector card, `listing::heard` and `COUNTS_AS_HEARD` use).
  `format::age` fits it, from the `SPANS` table `format::since` reads, short spelling: one `Span`
  carries singular, plural and brief, so a span cannot be named one way and forgotten the other
  (`every_span_is_named_both_ways_and_no_two_read_alike`). A row nothing played draws the empty
  cell; a count with no date (a playlist's own reading can be) draws the count alone.
- **A catalog listing marks the playing track by the row it resolves to, never the queue's id.** The
  tracks pane, Favourites and an opened suggestion compare against `RootView::playing_now`'s
  `track`, the library row `LibraryModel::track_of` weighs out of the queue row's location and span.
  They compared the queue row's own `TrackId` (the library's only for a row loaded from the catalog
  this run): a resumed queue is minted afresh by `Queue::restore`, so after a restart Play lit
  nothing. What `track_of` read is held in `named`, the albums the bar links to in `read_albums`,
  only until the catalog next changes: `LibraryModel::reload` forgets both, so a title, artist,
  album link, favourite or *not in the library* a rescan or lookup changed is read again
  (`a_queued_row_is_read_again_once_the_catalog_changes_under_it`).
- **The row playing is the row a list is on, never a track id.** A file can be in a queue or
  playlist twice and an unscanned row has no `TrackId`, so both panes ask which row the transport is
  on. The queue draws `PlayerState::queue_position` (row of the published play order); a playlist
  `PlayerState::loaded_position` (row of the load order: the playlist's own order however shuffle
  moved play order). A playlist marks a row only while `Library::playing_playlist` names it, i.e.
  while the queue holds the rows it was loaded with: sound exactly as long as the queue *is* that
  playlist; a row added or dropped (window or bus) ends it.
- **An edit never cancels the one before it.** `LibraryModel::edited_then` holds one task in
  `_edit`; replacing it dropped an edit spawned a moment earlier before it ran (write, toast, reload
  gone). Each new edit takes the previous out of the slot and awaits it first: edits run in order,
  none lost (`two_edits_asked_for_at_once_both_land`); `track_passed` and the two `listened` writes
  on `_settled` chain alike (a settled play's length no longer dropped by the next play's first
  hearing).
- **A narrower read never cancels a wider one.** A read replaces `_load`, dropping the one in
  flight: right between two whole reads, but it lost browse panes when a playlist edit's
  `ThePlaylists` read landed on a scan's closing `Everything` (the listing kept the rows the scan
  had just pruned). `in_flight` is what the model remembers; a read asked while one runs reads what
  both would have: `Wanted::with` = `Everything` over anything, two of a kind as they are,
  `TheSearch`+`ThePage` → `TheSearch`, `ThePlaylists` with `TheSearch`/`ThePage` → `Everything`
  (`a_read_asked_for_while_another_runs_covers_what_both_would_have_read`).
- **A read of the library says how much it wants.** `LibraryModel::read` takes a `Wanted`.
  `Everything`: browse panes, what stands whatever is typed (statistics, most listened, days,
  suggestions, roots: `Standing`), shelves (playlists, wants, missing: `Shelves`). `TheSearch`:
  browse panes and shelves, leaving the standing (a keystroke, selection, sort or favourite moves
  none of it, so a search no longer asks for the week's statistics each time) and the wants
  (`reads_the_wants`; a `WantsRead` rides in `Shelves` as an `Option`; no keystroke moves them and
  they read every want and its links per settled key). `ThePage`: the four lists `reach` bounds
  (albums, artists, tracks, scoped tracks: `Paged`) alone, for `reach_further`, their counts and
  every other pane standing still. `ThePlaylists`: shelves alone, wants included; merged with any
  other kind it is `Everything`, the one shape reading listing and wants. Every gesture behind
  `LibraryModel::edit` writes only `playlists`, `playlist_entries` or `playlist_queries`, so an
  edit, a listing-order change and opening a playlist take `ThePlaylists`; a scan, a counted play, a
  window of time and forgetting a root take the whole. `Loaded` carries each part as an `Option`, so
  `take` tells empty from never asked; `renewed` swaps a list's `Arc` only where what came back
  differs, so a read that moved nothing leaves every pane's rows, the album index and the chart
  where they stood (`a_list_read_again_the_same_is_left_where_it_stood`).
- **A read the typing asked for waits for the typing to stop; every other runs at once.**
  `LibraryModel::set_query` is the one caller giving `read_after` a delay, `SEARCH_SETTLE` (150 ms):
  ten characters cost one pass over albums, artists, tracks, the playlist index with its `count(*)`
  per saved query and the opened playlist's entries, not ten. A keystroke does move `Search::read`
  and the query beside it (a handful of tokens), so the *Reads* row and a pane's own narrowed
  reading are live as typed; only the SQLite work settles. The wait lives in the `_load` task, so a
  keystroke inside it drops the one before rather than queueing a second.
- **Every notice is a toast: a pill that rises over the playback bar, lingers and goes.** Cousin
  project wsg's `chrome::toast`; `toast.rs` is the whole of it. `Toaster` is a gpui global, not a
  model field, so engine failures, library passes, the equaliser and every gesture tell it alike
  (`toast::tell(notice, cx)` from anything holding an `App`); `RootView` redraws on
  `observe_global`. One pill at a time, centred over the content `type_ahead_lift` above the bar,
  lifted clear of the type-ahead pill when showing; rises `RISE` (10 px), fades in over `ARRIVAL`
  (220 ms), lingers `LINGER` (6 s; `LINGER_IN_A_BURST` 2 s where more wait; at most
  `WAITING_AT_MOST` (4) waiting, never the same words twice), fades out over `WITHDRAWAL` rising
  the same 10 px; a click or escape takes it early. The one surface with an exit; everything else
  that arrives follows `motion.md`. A `Notice` is `Trouble`, `Done` or `Noted`; the tone is the
  pill's icon (alert in the failure colour, tick in green, info mark in the accent), words staying
  body text colour. `EqualiserModel` sets its notice where it holds no `cx`: an outbox `RootView`
  drains into the toaster (`take_notice`) whenever the model notifies. One line stays in place,
  belonging to its control: the Library card's refusal of a typed layout field. **A finished pass is
  a toast too, never a line left standing.** A scan, organise, retag, vault import and inbox poll
  draw counts in their settings group only while running (`stats`, `poll_stats` answer `None` once
  over; a lookup draws none); the model tells what came of it as it joins (`scanned`, `looked_up`,
  `filed`, `retagged`, `vaulted`, `polled` in `models.rs`). A scan or poll the window started on its
  own is told only where it brought something (a watch rescan finding nothing stays quiet). A
  *preview* keeps its summary in the group: the plan the second press arms against, not news.
- **Toasts speak plainly; the error goes to the log.** No error `Display` reaches the pill. The
  engine names failures `resonate_engine::Cause` (`Unreadable`, `Unsupported`, `Damaged`,
  `NoDevice`, `DeviceGone`, `SoundServer`, `DeviceRefused`, `CannotSeek`, `QueueMoved`,
  `NothingPlaying`, `PlayerStopped`), read exhaustively off codec/PipeWire errors in `Error::cause`
  (window sees neither crate). `Error::location` names the failed file, so a row gone from the queue
  before the next poll is still named: *Couldn't find “Gone Song” — it may have been moved or
  deleted*. `toast::would_not_play`/`would_not_do` turn a cause into words; library/equaliser
  failures use `toast::could_not("start the scan", &error)` (*Couldn't start the scan — another
  library task is still running*; reason only where the typed error has one worth saying). A notice
  is a label: no full stop. `looked_up`: *34 albums and 12 artists answered* (breakdown left to the
  Online card); the sidebar enrichment line wears `faint`/`muted`, not accent.
- **No device name in the playback bar.** The sink description was the signal path's one child with
  no width of its own: a long one pushed the panel to its clip edge and took the album title along.
  The inspector's OUTPUT stage names the device; the settings row keeps its node name behind the
  pointer; the signal path's hint says *Playing through …* before its old text.
- **Thin root over four cached regions: a moving clock redraws only the playback bar.** gpui re-runs
  the root view each frame, reusing layout/paint only for a child drawn as a *cached* `AnyView`
  nothing notified. `views/part.rs`: a `Part` per region (header, sidebar, pane, bar) renders
  `RootView::region` via a weak handle (panes stay `RootView` methods on `Context<RootView>`,
  listeners bind to the root); `RootView::render` = shell (actions, overlays, menus, toasts). Each
  `Part` observes the root (`cx.notify()` on `RootView` redraws all four); a hover/scroll/frame
  asked inside a region notifies it alone (gpui names the view a listener was painted under). A
  cached view is laid out from a style, not content, so `Region::laid_out` declares sizes: header
  and bar fixed heights, sidebar its width, pane the rest in a one-cell grid, track `minmax(0, 1fr)`
  (never wider than its region whatever least width its content asks; a flex row honoured a pane
  root's automatic minimum).
  - `PlayerModel::refresh` reports a poll as `Moved`: `Clock` if only position and sink latency
    changed (the `held_still` reading `Grain::shows` makes), else `More`. Root observer: `More`
    notifies itself; `Clock` → `Parts::the_clock_moved` (bar, plus pane where
    `Pane::follows_the_clock`: inspector latency, analysis playhead). At scale 1.5 on 2880×1800 a
    playing window went 4.2 % → 2.4 % of a core (tracks), 6.8 % → 2.4 % (albums), 15.5 % → 3.2 %
    (inspector).
  - **`Grain`** (`EveryPoll`|`Stepped`) = how often the clock moves, told by the render to the
    model. Every poll: visualiser (spectrum follows), held seek thumb. Else a quarter device pixel
    of the seek rail (`STEPS_PER_PIXEL` over `Rail::width` × scale), never coarser than `CLOCK_STEP`
    (1 s): elapsed time turns over on time, and `Listening`/`Keeping` never see a step near the
    `A_SEEK` they read as a seek; unknown length = `CLOCK_STEP`. Lyrics is not every poll: a synced
    set asks a frame at display rate while it moves, an idle pane has nothing a poll would change
    (forcing one was a whole-window paint 60×/s over an empty pane). `Grain::shows` passes anything
    but position/latency at once (pause, seek, track change, sleep timer's second); an unpainted
    rail is every poll. Model state still refreshes every poll, so whatever else asks a frame draws
    the moment as it is.
- **Playback bar = three columns, transport in the middle.** Now-playing panel and status cluster:
  `flex_1`, zero basis, ≥ `SIDE_AT_LEAST` (180 px), equal halves of what the centre leaves, so the
  buttons sit on the window's centre line. Centre: basis `theme::transport_centre()` (460), shrinks
  to `CENTRE_AT_LEAST` (260), gives way first (460 at a 720 px window left each side 86 px: cover
  took the panel, title/artist/album drew nothing). Holds step and play buttons over the seek rail,
  elapsed and total clocks at its ends; a press on the total toggles time left (`-3:12`,
  `RootView::showing_time_left`, not kept between runs); rail fills its row
  (`Handle::fills_its_row`). Both sides `overflow_hidden` (a wider badge or notice would paint over
  the controls). Engine notices are toasts, in neither half.
  - **Seek rail names the time under the pointer** in a bubble above the track (`slider::Pointed`,
    `RootView::seek_pointed`): the rail's `on_mouse_move` writes the fraction; `on_hover(false)` and
    the pointer leaving the window clear it; a held thumb names its own. Drawn, not a `hint::Names`
    tooltip (built once where raised; this follows the pointer); nothing where length is unknown.
  - **What does not fit is left out whole, never clipped.** `views/gives_way.rs` = arithmetic over
    theme measures, no gpui. Status cluster measured into `RootView::status_room` a frame behind (as
    `playing_room`); `status_kept` drops volume reading, sleep control, repeat, shuffle, then volume
    rail, until the rest fits (running timer weighed at its countdown's width); never the queue
    button (the one way to the queue pane) or the speaker (still mutes, takes the wheel).
    `signal_kept` likewise against `playing_room`, widths via `kit::width_of`: mode's name first,
    then depth and rate, then the mode's dot; codec badge stays. Unmeasured column keeps everything.
    (At 960 px the cluster had clipped from its left edge: queue, shuffle, repeat gone, the moon cut
    in half, the path cut mid-figure.)
  - **Now-playing panel**: cover (`theme::now_playing_cover()`), title, artist, album, signal path:
    decoding-format badge with depth and rate, then the output's mode dot and name in the mode's
    colour (*bit-perfect*, *converted*), nothing after (a link mark and negotiated depth/rate once
    doubled the mode name and the inspector's *Output* stage; what a converted stream became is the
    inspector's to spell out). A press opens the inspector, whose three stages expand this line.
  - **A skip never blanks the panel.** While the next row opens the engine publishes no current
    track (`Buffering`, or `Paused` for a skip made paused: `between_songs`): `RootView::playing` answers the song last resolved,
    `name_the_window` keeps its title (no *Nothing playing* flash), `held_through_a_change` draws
    the last cover (`RootView::shown_cover`) up to `COVER_HELD_FOR` where one is expected, not the
    disc. The signal path is held too (`held_signal`), so the text column above never moves; changes
    are handed over, not redrawn (`motion.md`: crossfade).
  - **Title, artist, album are three elements**, each opening its page via `RootView::opens`:
    title/album scope the tracks pane to `Selection::Album`, artist to `Selection::Artist`. A scope
    whose album/artist the catalog no longer holds (`Browsed::gone`) becomes `Selection::Everything`
    when the reading lands, not an empty list headed by its number. Pressable = *known*: album id
    rides on the resolved `Cover`, artist id on `Track::artist_id`; an unscanned row draws all three
    plain (tag names, nothing behind). `RootView::by_line` = artist, separator, album as one method
    taking the two element ids and the room, so the inspector's heading is the same line (lyrics,
    visualiser, analysis headings use `RootView::playing_heading`: title and artist measured into
    `heading_room`). `keeps_its_width` holds the artist at `flex_none` under `max_w_full`: three
    truncating children otherwise shrink in proportion to length (an album 4× the artist's took a
    quarter of the loss from the artist); now the album gives way, the artist ellipsises only where
    it alone overruns.
  - **What gives way ends in an ellipsis, not a square cut, each given a width it can be cut at.**
    gpui's text element truncates only inside its measure and keeps a no-wrap text's first measure:
    a `truncate`d name in a content-sized flex item is laid out whole and sliced by the parent's
    clip; a `line_clamp` there is measured at min-content width and drew *The Tr…*, or nothing.
    Title and album go through `kit::cut_to_fit` (gpui's `LineWrapper::truncate_line` on the text
    itself). Title: cut at `RootView::playing_room` (a frame behind) less the star. Album: at the
    room `by_line` is handed less artist and separator (`kit::width_of` sums font advances; a
    glyphless character, CJK/emoji, counts one em, what the fallback draws); no room left = artist
    alone. The inspector measures its heading into `inspected_room` likewise. The album once was
    `flex_1` under `ends_in_an_ellipsis`; gpui 0.2.2 paints a clamped line's underline over the
    whole unwrapped run (a one-line clamp records no wrap boundary), so hovering an overlong album
    underlined the rest of the panel. Inspector stage cards are `flex_1` already and take the clamp;
    their row wraps (a pane unable to hold three at `stage_width` puts the last under the first
    two).
  - **Every bar control names itself via `views/hint.rs`** (a silhouette does not say what it does):
    a step names its key, a toggle its state and what the key does next (accent is not the only
    report). A rail is filled in `TEXT`, accent under the pointer or held, thumb only then. The
    volume reading is always drawn, in `theme::volume_reading()` of room and the body face (at
    `opacity(0)` until the pointer was over the cluster it reserved room unfilled, a hole between
    rail and window edge). `kit::readout` draws it; `theme::ui` = body face with `tnum` on: as
    steady as `kit::figure` without being mono (a percentage beside a slider has no reason).
- **The play mark carries its optical nudge in the art, not a margin.** A right-pointing triangle
  centred by bounding box reads left (mass on the base), so `play.svg` (`M7 5.5 18 12 7 18.5Z`) sits
  half a unit right of the 24-box centre; the button adds nothing. It replaced a 2 px `margin-left`
  on the play state alone (triangle ~2.5 px right of the disc's centre; `pause.svg` dead centre with
  no margin, so the mark moved sideways on every toggle). What an icon needs to sit right belongs in
  the icon.
- **A lyric look starts when the track does, not when the pane opens.** `look_for_lyrics` runs on
  `RootView`'s observer of `PlayerModel` (the one `count_a_play`, `Keeping`, `Following` ride): a
  track change searches whatever pane is in front, so the pane opens on a set, not *Looking for
  lyrics…*. Idle cost nil: `Asked` is `Copy`, `worth_looking_again` refuses a repeat, the look runs
  on the background executor.
- **The whole sheet opens out wherever the pointer is on the column.** `near_the_words` (bounds in,
  `bool` out, testable without a window): inside the centred column band (+ `Measures::pad` slack),
  pane top to foot. It once answered only within reach of the line being read, leaving sung lines
  dark under a pointer held over them, exactly where one points to read back. The view uses the
  `pointer_watch` idiom: a zero-size `canvas` registering a `MouseMoveEvent` window listener from
  *paint*, the one phase `Window::on_mouse_event` may be called in. An `on_hover` on the pane opened
  the sheet from the empty gutters and either far end.
- **The lyrics pane follows the playing track and nothing else.** `RootView::wanted_lyrics` builds a
  `Wanted` from the queue row's location and `StreamDigest` tags, filling title, artist, album,
  length off the scanned row until the digest lands (album via `LibraryModel::album_title`, length
  at the row's own rate), so the first look has all a provider searches on and a `[length:]` to
  weigh a sidecar against. A redraw compares `lyrics::Asked` (track id, duration, whether a digest
  for it landed; `Copy`, all that could change what is wanted); `Wanted` is built only once
  `LyricsModel::asks_again` says the reading moved (a 60 Hz redraw once cloned title, artist, album
  and the whole embedded lyric text out of the digest to find them unchanged).
  - `follow` resets the pane only where the *track* moved, so a digest landing mid-track refines the
    look without rewinding the scroll; the lookup it starts rewinds on landing only where the track
    moved or it answered a sheet other than the one shown (`already_shows`). `worth_looking_again`:
    asked again on a track change, else only where `Wanted` differs from the last searched. `Asked`
    moving is not enough (`tagged` flips once per track as the digest lands, so every scanned track,
    whose row already gave the title/artist the digest repeats, paid a second search that could only
    repeat the first). An unscanned track still pays two and earns them: first on location alone
    (finding a sidecar), second on what the file says it is. The look runs on the background
    executor (a network provider must not block the frame). Lines become `SharedString`s once, when
    the look lands: a 60 Hz redraw hands each a reference count, not a copy of the sheet.
- **The lyrics heading says where a set came from; what it says of itself rides behind the
  pointer.** `attribution` ends the heading: a `SYNCED`/`UNSYNCED`/`WORD-SYNCED` badge, the
  provider's name as a `kit::figure`, and, only where the sheet credited anyone, one more figure for
  who laid it down: the most particular claim, `[by:]` transcriber, else `[au:]` author of the
  words, else the editor; nothing where none is named. The rest is that figure's `hint::Names` hint:
  words' author, sheet's, editor with `[ve:]` version, `[al:]` record, each sentence only where the
  sheet wrote it. One figure is all the crowding the row takes (Follow, two reading chips, the press
  hint, two attribution marks already in it): a sheet declaring everything reads as one name, hovers
  as four sentences.
- **The pane always draws and scrolls the whole sheet; a reading is only how far the light
  reaches.** `Falloff::Around` looks only forward (line sung + two after; everything sung out) =
  `Reading::InPlay`; `Falloff::Across` steps down gently both ways (tail 0.16) so `Reading::Whole`
  stays readable to the edges, type bold, centred. A line is drawn for what is coming, not what has
  been (the last one kept behind the sung one reads as a transcript, not a track playing):
  `Falloff::behind` is `0.0` for `Around`, `ahead` mirrored for `Across`. Drawing every line either
  way makes the motion possible (an `InPlay` reading building a fresh three-line column each time
  would have nothing to scroll; lines would swap, not slide). The choice is a heading chip living
  only as long as the run (like the settings pane's category); the pointer over the pane opens it
  out, as does a scroll of your own while it holds the pane: `LyricsModel::shows_every_line` is the
  one answer all three use. It weighs `following`, which runs out on a clock, so `follow_the_track`
  turns `spread` onto it every frame and the sheet closes again over one `TURN` once `HANDS_OFF` (6
  s) is out. An unsynced set has no lit line: every line at `ADRIFT`, no chip.
  - **The sung line brightens; it does not grow.** Every line is `Measures::words` in a box of
    `Measures::leading`, lit or not: a row takes the same rows at the same height whatever the turn
    does. A size in motion cannot be drawn smoothly: gpui on Linux floors a glyph's vertical origin
    to a whole pixel (`SUBPIXEL_VARIANTS_Y` is 1) and cosmic-text hints every size (Inter carries
    TrueType bytecode), so the line growing 26 to 36 px was re-hinted at each of eighty steps (cap
    height and baseline on different pixels: a shimmer through the turn, a one-pixel hop at the
    end). Earlier, a row was measured at the size it would reach and its text laid out in a box
    scaled by how far it had grown (a line on one row at rest took two as it lit and slid the sheet
    under itself).
- **Two voiced lines have separate reading edges.** A second voice puts voice one on the leading
  side, voice two on the trailing side; alignment alone says which voice sings (no label); the
  second also takes the accent when lit. Each voice's active line brightens on the existing turn,
  even where both sing together. A one-voice set keeps its centred column and former width.
- **The falloff counts written lines, not rows.** `drawn` records each line's ordinal among
  non-blank lines when the look lands: a blank line between verses costs its neighbours no standing,
  and the reading is the same three lines across a verse break as within one (per-frame computing
  would be a walk per line per line).
- **Three `Turn`s on one `TURN`, and a `Glide` on a cubic.** Where the pane *reads* and what it
  *lights* are different lines, each with its own clock. `Reads` is what the light turns on: `At` a
  line while one is in play, `Spent` where none is (carrying the line `read_at` names), `Evenly` for
  an unsynced set (never lights one). `standing` blends over `Reads`, `lead` over the lines in play;
  colour reads off `lead` (`mixed` lerps `muted` to `text`), so a line brightens over one 420 ms
  `TURN`, no threshold snap. The third turn is `spread`, over the `Falloff` itself: the pointer
  opening the pane out and a reading chip both go through `Turn::onto`, so lines a wider reading
  brings up fade in over the same span; `standing` = the falloff turn blended over the reads turn.
  - **Nothing lit is not an empty pane.** Where nothing is in play but no wait has begun (a gap
    under a breath, a line whose sheet ended it just short of the next), the turn stays `At` the
    line just read: it keeps its standing, only its colour falls to `muted` as its light goes
    (`Spent` there put every line out for the gap's length: text vanishing for a second between two
    singers). `Spent` is a wait's: the set goes to `Falloff::spent`. `InPlay`: the line waited on
    and the one after stand as though the line before were being sung (`ahead(1)`, `ahead(2)` under
    the dots, nothing behind). `Across`: every line takes the same step down either way from the
    read line as a line in play gets, that line standing where its neighbours do, not lit (a
    deliberately asked reading stays readable). Everything out left a pause as dots alone, the line
    rising out of nothing; a flat 0.16 over the lot before that left the sheet all but unreadable
    past the last line however opened out
    (`a_gap_shorter_than_a_breath_holds_the_line_just_sung_rather_than_putting_the_sheet_out`,
    `a_pause_leaves_the_line_it_waits_on_and_the_one_after_readable_under_the_dots`). `read_at`
    keeps the sheet where it is, which also takes a set's last line away once it has had its word,
    not leaving it lit through the outro.
  - **The `Glide`** differs in kind: it carries the scroll offset to the read line's `landing` over
    `GLIDE` (600 ms) on `landed`, `1 − (1 − s)³(1 + 3s)`: at rest at both ends, fastest a third of
    the way, never passing its landing (decelerating into it reads as the sheet arriving;
    ease-in-out as being pushed). A cubic, not an exponential: on whole pixels the last pixel lands
    ~35 ms after the one before. The critically damped spring it replaced crept its last pixel or
    two in 130–150 ms apart, long after the sheet looked still (a jump at the last moment); an
    underdamped one before it came back from its overshoot the same way, as a flutter
    (`a_glide_lands_on_a_whole_pixel_with_its_last_step_close_behind_the_one_before`).
- **The sheet grows with its pane; its type is still a size at rest.** `Growth::of` takes the
  tighter of the pane's width/height ratios against `PANE_AT_RESTING_SIZE` (784 × 600: 720 px column
  + two gutters, ~ten rows), held in 1..`GROWS_AT_MOST` (2.5): a wide short pane grows no type it
  has no rows for; at or under resting size it draws as always. Every sheet length is
  `Measures::at(scale, growth)`: type, end-mark label, leading, padding, spacing, breath room,
  pause, plain margin, column, gutters, line side padding, dots, end mark, dissolving edges, how far
  a set rises (`Measures::rise`); Text size setting's scale × growth, so the setting keeps its
  meaning on 4K. Growth changes only with the pane: layout's, not motion's (a turn never changes a
  size). A resize moves `Measures`, which `place` weighs, so every line is laid out again before any
  is held open at an old height (`the_sheet_grows_with_its_pane_by_the_tighter_of_its_two_sides`,
  `the_column_widens_with_the_pane_and_never_outruns_its_gutters`).
- **Everything in the sheet moves on whole device pixels (gpui draws text on them).** A glyph's
  vertical origin is floored while a quad is drawn where it is: fractional text stepped a pixel at a
  time while its hover wash and dots slid. `Scale::snapped` rounds to device pixels
  (`RootView::follow_the_scale` hands the lyrics its scale). `landing` is snapped before a glide
  leaves (an odd pane height put a centred line on a half pixel, crossed only in the last frame);
  `glide` sets the offset snapped; `inset` rounds each line's place *once*: `snapped(line) −
  snapped(sheet)`. Offset and lag rounded apart (glyph floor vs taffy) made their sum tick back and
  forth though both moved one way: 322 reversals over a minute of a test sheet, read as vibrating
  lines (`every_line_is_drawn_on_whole_pixels_and_never_steps_back_through_a_glide`). `Measures`
  holds every vertical row length (type, leading, padding, spacing, breath room, pause, end-mark
  padding) snapped the same way at every growth, and the head is `edge()` snapped, so every row
  starts and ends on a whole device pixel. taffy rounds a size by position (`round(top + height) −
  round(top)`): a 62.08 px row came out 62 or 63 as rows above changed, moving the read line a pixel
  with no line change (`the_measures_of_a_line_are_whole_device_pixels_at_any_scale`).
- **A line further down sets off later: a change ripples, not shifts.** Each row is two boxes: outer
  = what `bounds_for_item` measures; inner = `relative()` with a `top` inset of
  `LyricsModel::inset`, so motion never moves the bounds the landing is computed from.
  `Glide::lagging` is the whole ripple: a line `n` written lines past the read line is where the
  sheet was `LAG_PER_LINE × n` ago (capped at `LAGS_AT_MOST`), its inset = distance from there to
  the sheet: lines below the sung one still catch up as it lands (as Apple Music's sheet; one offset
  for the lot cannot). Lines above lag nothing (sung lines get out of the way first).
  `Glide::settled` waits the last lag out, so frames keep coming until the furthest line is home.
- **A set arrives rather than appears.** `place` starts `arrived` when the pane is placed; two
  readings: `arrival` (body opacity, an ease over `ARRIVES_IN`) and `rise` (each line's inset,
  `RISES_FROM` down, falling to nothing on `landed` over `RISE`, `RISE_PER_LINE` later per written
  line from the read line either way, capped at `RISES_AT_MOST`). Until placed `rise` answers the
  full `RISES_FROM` and `arrival` nothing: first frame low and invisible, the set comes up out of
  the read line. `breath` reads the same clock: a sine over `BREATH`, what the gap's dots swell on.
- **A look only just started is kept quiet.** `follow` stamps `asked_at` where the track moved on;
  `looks_quietly` is true for `LOOKS_QUIETLY_FOR` after while still `Searching`: the pane draws an
  empty body, not *Looking for lyrics…* (a sidecar answering in milliseconds and a placeholder
  flashing between sets is the least seamless thing about a track change). It asks its own frames,
  so a longer look is announced when the grace runs out.
- **A gap is read at the line it waits for, not the one just sung.** `read_at` prefers
  `Waiting::next`, then the ended row, then `line_at`: an instrumental scrolls the upcoming line to
  the middle and lets it rise there under the dots while all sung is out; past the last line,
  nothing to wait for, it falls back and holds.
- **`centre_of` reads the laid-out bounds; a new landing carries the glide on.**
  `ScrollHandle::bounds_for_item` is gpui's layout with no scroll offset, so the landing *is* the
  offset (adding the current one would make the target chase the glide, never settling). A landing
  within `SETTLED` (half a pixel) of the held one is no move; any other `redirected`s the glide: a
  new `Leg` from where the sheet is, carrying its pace as `launch` on `launched`, `s(1 − s)³`
  (starts at that pace, dies on the same cubic tail), held to `LAUNCHED_AT_MOST` × the travel so it
  never passes its landing. The leg before stays as `Glide::before`, so a lagging line reads the
  path the sheet took. Lines sung faster than a glide scroll as one motion (a glide restarted from
  rest stalled at every line and each lagging line jumped its whole lag as it restarted)
  (`a_new_landing_in_flight_carries_the_glide_on_without_a_jump_or_a_stop`).
- **An unsynced sheet is placed at its top once, then left to the reader.** With no line read,
  `landing` answers the top for a synced set (waiting on its first line) and any set not yet placed;
  a placed unsynced set answers nothing, so `place` leaves the offset where the wheel took it
  (answering the top glided a plain sheet back to its first line `HANDS_OFF` after every scroll)
  (`a_plain_sheet_is_placed_at_its_top_once_and_then_left_where_it_was_read_to`).
- **A line far from the pane is held open at its height, not laid out.**
  `LyricsModel::resting_height` answers the height `bounds_for_item` last gave a line lying more
  than `DRAWN_WITHIN_PANES` (a pane's height) above/below the pane (scroll offset added, which the
  bounds leave out), only while `LyricsModel::steady` (pane size and `Measures` the same two frames
  running). A frame reads the layout before it, and a line held open in that layout was read off the
  one before, so a change must lay every line out once more before any is held again (gating on the
  frame's own size let the first layout, pane zero wide, a letter a row, lines a thousand pixels
  tall, hold those heights for the sheet's life)
  (`a_line_is_held_open_only_once_two_layouts_running_were_measured_alike`). `lyric` draws such a
  line as an empty box of that height and width, so layout, the child indices `centre_of` reads and
  every other line's place stay put while a hundred unseen lines cost a box each, not a text layout
  per frame. A line coming within a pane of view is drawn whole before it can be seen; a pane
  changing size lays every line out once more.
- **The end of the words is a row of its own: an outro is not an empty pane.** Once
  `Lyrics::has_ended` (nothing in play, only blank lines to come) the read line is
  `LyricsModel::end_of_the_sheet` (one past the last) and the turn reads `At` it: the sheet glides
  to an *END OF LYRICS* mark between two short rules (`end_of_the_words`, last child before the
  trailing spacer), lit while all sung falls away as a line behind the sung one does.
  `Sheet::written` carries one more ordinal for it, so it rises, lags and stands like any line:
  faint two lines ahead of the last word, lit once it has gone out. An unsynced set draws none (no
  clock to end on).
- **A pane is drawn nowhere until placed; a row is as wide as the pane says.** The sheet's spacers
  are measured off `ScrollHandle::bounds`, a frame behind: first frame no padding at offset zero,
  second with padding but centred off the first's child bounds (two frames of a sheet sliding into
  position = "it glitches and then corrects itself"). `place` reveals nothing until `steady` (size
  and `Measures` of the layout are current), snaps the offset instead of gliding, and reports itself
  moving throughout so frames keep coming while paused; until then the body is `opacity(0)`, laid
  out unseen. `follow_the_track` snaps its turns over the same span, so a set arrives at its
  standing, not fading in from `Reads::Evenly`. `rewind` puts it all back: a track change places the
  next set afresh, not gliding from where the last sat.
  - Each row takes an *absolute* width, `LyricsModel::column_width` (pane's measured width less
    `Measures::gutter` each side, capped at `Measures::column`), not `w_full` under a `max_w`: taffy
    fixes a flex item's height from a measure taken before a percentage width resolved and never
    re-measures, so under `w_full` every wrapped line was laid out one row tall while gpui painted
    it wrapped, the next row drawing over its second.
  - A synced sheet's head is the container's top *padding*, its tail a spacer *child*, not two
    half-measures: `content_size` is the children's extent alone, so a leading child would put every
    line a row off what `bounds_for_item` answers and leave the sung line a line's height below the
    middle, while a trailing padding would leave nothing to scroll into and the last line never
    reaching it.
  - The pane asks its own frames (a zero-size `canvas` calling `Window::request_animation_frame`
    while any clock is in flight); the 16 ms poll (`POLL_INTERVAL`) notifies only when the player's
    state *changed*, so a turn started by a press, seek or chip while paused would freeze half way.
  - **The poll rests while nothing moves**: `PlayerModel::refresh` answers `Poll::Quiet` when the
    player is not playing, no sleep timer counts down, no seek is in flight and nothing changed;
    after `QUIET_POLLS_BEFORE_RESTING` (30) quiet polls the loop waits `RESTING_POLL_INTERVAL` (64
    ms) instead of `POLL_INTERVAL` until the next change: a paused window still answers a press
    within a frame or four at a quarter of the wakeups.
- **A sheet that runs early or late is moved by the listener, for that track.** A synced set's
  heading carries *Later*, *Earlier* (`LyricsAhead::STEP_MS`, 100 ms a press, held within
  `MOST_MS`, 30 s) and, while moved, the offset as a selected chip putting it back
  (`said_ahead`: *+0.3 s*). `RootView::look_for_lyrics` reads `Library::lyrics_ahead` for the row
  it builds a `Wanted` for and hands it to `LyricsModel::hold_ahead`; `nudge_the_lyrics` writes
  `Library::hold_lyrics_ahead` against the row the model looked for. The pane reads every line at
  `ahead.read(clock)` (positive: the words come sooner), and a line press seeks to
  `ahead.back(moment)`, so the press still lands where the words are sung.
- **A line press seeks; your own scroll is not fought.** Timed lines call
  `RootView::seek_to_moment`, clamped to the duration before `Command::Seek` (engine refuses a frame
  past the end; an overrunning `.lrc` stamp left a red bar line until a track change). A wheel sets
  `led_by_hand`: no following for `HANDS_OFF`, heading shows *Follow*. That press, a line press or a
  track change resumes.
- **The pane reads its own clock, not the engine's position.** `PlayerState`'s position (decoded
  less ring and device) climbs a block at a time (~90 ms FLAC), draining in quanta: a sawtooth
  stepping word sweep, filling dots, line lighting. `LyricsModel::keep_time` runs a wall-clock
  `Clock` from the last sample, steered to each new one by `1 − e^(−t / STEERED_OVER)` of the way
  (`t` = time since last: a share of time, not of a frame, so 240 Hz steers like 60 Hz), never back
  past where it stood. Seek (`Seeks` moved), new track, pause or drift past `DRIFTS_AT_MOST` takes
  the published position as is. A playing synced set asks every frame on the display's clock (the 16
  ms poll beats against it).
- **A gap breathes, mid-song and before the first line.** `Lyrics::waiting_at` answers only where no
  line is in play: line waited on and how far through, counted from track start before the first
  line, else from when the last line went out (`lyrics.md`: when a line goes out, why a blank line
  is a pause). In `resonate-lyrics` beside `LIT_AT_MOST` (one ten-second constant). Three dots stand
  where that line will be, filling in turn, swelling on `LyricsModel::breath` by `DOT_SWELLS_BY` of
  `Measures::dot`; the line rises towards lit as the count runs out. `breather` is a canvas of the
  *fully swollen* size, each dot a quad round a fixed centre (dot-sized layout shifted every line;
  taffy rounded boxed dots to whole pixels, so they wobbled). **The dots never change the layout:
  their room is the sheet's, not the wait's.** `Lyrics::breathes_before` gives from timing alone the
  lines a wait counts down to (first written line; any whose previous written line goes out
  `A_BREATH_AT_LEAST` ahead); each has `Measures::breath` of room above its text for the sheet's
  life (reads as the gap between verses). Dots only fade there: `Breathing` in over `TURN` as a wait
  begins, `FadingBreath` out over `TURN` as it ends on its line. `landing` centres a line's text,
  not its row (half the room off), so lines with and without room read at one height. (Dots as a
  child jumped every line below; easing their height collided with the next line.) Progress
  *through* a line only where the set times words (`lyrics.md`: the sweep); a rail under a
  whole-line-timed line fakes a sweep.
- **A name the window can open is an `opens`; its id decides.** `RootView::opens(…,
  Option<Selection>)` wraps `pressed_to`: `Some` = link (accent+underline under pointer, cursor,
  hint, listener selecting and showing `Pane::Tracks`), `None` = same truncating element without
  them (one call site for scanned and unscanned rows). Every pressed name uses it: bar
  title/artist/album; inspector and lyrics headings; owner under a scoped album heading; artist
  under a grid cover; artist cell of every row in tracks, queue and playlist listings. The listener
  stops the press (a row is a press: the artist opens without starting the track, as `row_controls`'
  queue/playlist marks stop theirs). The link is `flex_shrink` under `ends_in_an_ellipsis` in a
  cell, never grown, as wide as the name (rest of the column is the row's); a longer name is shrunk
  and re-measured at that width, which draws the ellipsis. `listing::Row` has `artist_id` beside
  `artist`: filled by `listing::scanned`, `None` from `read`/`unread` (unseen queue row: artist as
  text). `unheld_row` passes `None` outright (release track, no catalog row), still through `opens`:
  one cell function.
- **A name past its room ends in an ellipsis; `kit::EndsInAnEllipsis` is the one place that knows
  how.** Bare `truncate` (`overflow_hidden`, `whitespace_nowrap`, `text_ellipsis`) cuts silently: a
  fixed cell slices the last letter; under the album grid's caption (content-sized flex item) no
  ellipsis came, taffy measuring under `MaxContent` and gpui's text cache answering that for every
  later measure of `nowrap` text, so the line lays out whole and truncation never runs.
  `ends_in_an_ellipsis` = `whitespace_normal` + `line_clamp(1)` after `truncate()`: a single-line
  *wrap* escapes the cache (measured with the width it has), the clamp draws the ellipsis;
  `truncate` supplies overflow, nowrap, `text_ellipsis`. `opens` takes it, so every link above ends
  in one, as do the grid caption and Missing headings. **No `truncate` stands alone** except two in
  `views/search.rs` (quoted query inside `kit::title`, which clamps itself; a section's faint hint
  line): all others (inspector values, menu entries, sidebar tab, pressing's line, device label,
  settings row) are `truncate().ends_in_an_ellipsis()`, so a name whose room ran out because an
  *ancestor's* did still ends in one. **A content-sized box does not wrap.**
  `kit::KeepsItsWidth::keeps_its_width` (`flex_none`, `max_w_full`) sets `whitespace_nowrap` back
  over the clamp: taffy measures such a box at min-content width, and wrapping clamped text drew
  nothing (bar title/artist/album blank every track once `opens` took the clamp). Those boxes are
  cut by `kit::cut_to_fit` where they must end in an ellipsis; a `flex_shrink` cell keeps the clamp.

## Driven by tests

- **A pane is pressed, dragged and scrolled by a test under gpui's test platform.** `resonate-ui`
  has gpui `test-support` as a dev-dependency; `driven.rs` is the harness. `Driven::open` sets the
  `ResonateApp` global as `run` does (`Player` over an `Unplugged` backend: no sinks, refuses every
  stream; in-memory `Library`; `Ephemeral` settings; `Places` in a temporary folder) and opens a
  real `RootView` in a `VisualTestContext`, activated (gpui reports focus only in an active window).
  It presses, right-presses, drags, scrolls and moves the pointer at bounds gpui drew, read via
  `debug_selector` (compiled out without `test-support`): `kit::Found::found_as` names every button,
  chip, segment, switch, choice and mark by `ElementId`; rows, tabs, menu entries, surfaces name
  themselves (`tab-visualiser`, `track-<id>`, `queued-<id>`, `menu-entry-<n>`, `picker-<id>`,
  `equaliser-curve`). `Driven::settle` runs the executor until it parks and advances the test clock
  a frame (model polls run); `Driven::until` waits on the real engine and scan threads;
  `Driven::play` opens rows via `Command::Resume`, the first paused, no sink asked of the graph.
  Driven: a tab, the visualiser's Scope segment, a track's menu and entry, the playlist picker, a
  queued row dragged below another, a suggestion card, the equaliser curve pressed/dragged/
  right-pressed, the settings body scrolled, the Listen sheet's segments and microphone chips
  (planted via `ListenModel::hearing_of`; microphones are PipeWire's to list), window drops, search
  typing and paste, the way back/forward, settings fields, the history switch, and the analysis
  pane's *Take this name* (`Driven::recognising` hands the window a `Fingerprinters` whose fake
  printer hears the playing file as another song; the press renames the catalog row:
  `the_name_the_audio_was_heard_as_is_taken_by_the_press_that_offers_it`). `views/root.rs`
  drives keys: the queue's reach dropped from its end, backspace after a lapsed jump, each sheet
  holding the pane's keys back, escape taking a menu before a toast, `ctrl-m`, `ctrl-u`, two
  playlist edits in one breath. **Found**: pressing the curve with the equaliser off turned it on,
  showing the switch's note above and moving the curve from under the press; the put-back mark
  appearing in a group's header grew the header. The note is now always there and
  `kit::section_header` holds a row control's height, so a first band lands where pressed.

## Panes

- **A favourite is a star; the heart is taken; missing is neither.** `Icon::Want`/`Wanted` (outline/
  filled heart) are both on a track row, so a second heart is indistinguishable. The heart is the
  *gesture* of wanting a row only: Missing pane, its empty state and an artist's *not held* button
  use `Icon::Missing` (disc, rim dashed away; a heart would read as a second favourites list).
  `Favourite`/`Favourited` = star, same 24-unit box and stroke weight. The mark hides on hover like
  every row control **unless the row is a favourite** (the one state legible at rest). Everything
  that fades must be inside what fades (an album cell's tinted ground was a wrapper the opacity
  never reached: grey square on every cover): a conditional control's ground, border and padding
  belong inside the condition. **A favourite is filled in the accent** (`kit::star`; unmarked = the
  grey outline of every row control, lit on hover): a fill alone was invisible in grey at 14 px.
- **The star says what was favoured from the press on, not the next read.** It drew the last catalog
  read, and bar and queue rows read `LibraryModel::named`, a cache nothing re-reads: a press wrote
  the favourite, the star stayed empty, the next press (still reading *not a favourite*) wrote it
  again. `LibraryModel::favour` records the write in `favoured` (`Favoured` -> answer), puts it into
  the `named` row where the track is cached, and notifies before the write runs. `favours` is what
  every star, cell and menu asks (own row's read the fallback); `favoured_album`/`favoured_artist`
  go through it, `favours_track` reads a resolved `Track`. **Bar, queue and an opened playlist ask
  it too**, their row from `named` keyed by the *queue's* id (resumed queue, unscanned playlist row,
  doubled row carry ids minted apart from the catalog's: patching `named` by the catalog's id missed
  the row the bar drew, star empty until the next run). `listing::scanned` takes the answer, not
  `Track::favourite`. The map lives the run, never wrong within it (every favour goes through
  `favour`). A failing write removes its entry: `edited_then` hands its closure `Edited::Failed`,
  `unfavour_what_did_not_save` drops the key and restores the replaced `named` row before the
  reload, so the star falls back to what the catalog kept and the next press sends the opposite of
  what is stored.
- **Three panes, each the same six edits.** `Pane::Favourites`, `Pane::Suggestions` under
  `Section::Collection`; `Pane::Statistics` under `Section::Library`. Each: a variant, a place in
  `Pane::BROWSE`, arms in `label`/`about`/`section`/`icon`, an arm in `RootView::content`, a sidebar
  count, a `mod` line. Favourites stacks artists shelf, albums shelf, ordinary track rows; its count
  is all three (only favourite albums would read empty). Its track rows have their own order,
  `Sorting::favourites` (*Favourited* newest first until a header is pressed; the tracks pane's
  would light that pane's order and sort the wrong list); `sorting::favourites_sorted` is its own
  `Sorted`, `#` column `Sortable::Marked` (when marked), not album order. Shelves stay in marked
  order.
- **The statistics chart is `div`s, not a canvas.** `inspector::traced` and the equaliser curve are
  continuous; this is a few dozen discrete bars, and a `div` carries a `hint::Names` hover free
  (canvas: hand-written hit-testing). A day with no plays still draws its baseline (no holes); a run
  over `BARS_AT_MOST` (120) folds whole days into one bar. Axis: *today*, *yesterday*, else the date
  (*29 Jan*, year where not this one) via `resonate-core::CivilDate` of the day `Calendar::local`
  puts each bar's midnight and now on (the calendar history buckets plays by: bar and date agree).
  `Chart::of` reads the local calendar, `Chart::in_calendar` takes one, so tests hold the axis to
  UTC and a zone east of it (`a_day_is_dated_in_the_listeners_calendar_rather_than_greenwichs`).
- **A suggestion card reads its rows off the frame.** `RootView::with_the_rows_of`, sibling of
  `with_everything_listed` (a suggestion may name the whole library): Play and Add to queue read on
  the background executor first. *Save* = `Library::save_query` through `LibraryModel::edit`, a
  taken name a `Notice::Trouble`, not a panic; a card whose search a playlist already fills greys a
  check under `kit::Press::Greyed`, no second copy.
- **Suggestions: shelves by kind; a card opens onto what it would hold.** `SuggestionKind` groups
  cards under *From your listening*, *Eras*, *Genres*, *Artists*, *Sound* in `SuggestionKind::ALL`
  order, each an eyebrow over a wrapping row. Pressing art or name =
  `LibraryModel::open_suggestion`: `SavedQuery` kept in a `Previewed`, first `PREVIEWED_AT_MOST`
  (500) rows read on the background executor; `preview_further` grows the window by as many again
  whenever the list is drawn within `LOOK_AHEAD` of full (larger prefix of the same ordered read, as
  `reach_further` for a browse pane), so a whole-library suggestion scrolls to its last row. While
  the opened query is still offered the pane draws it instead of the shelves: a way back; art at the
  text column's height via `Drawn::OnThePage` (an album cover's square); beside it, on the art's
  bottom, kind, name, reason, count and length, the search as *Reads* chips; under that Play,
  Shuffle, a save mark, a search mark; below, an ordinary unsorted track listing, each row playing
  the list from itself. Shelf card: art at full card width (still the grid's texture), name, then
  Play, queue and save marks on one row at the card's bottom (a wrapped title leaves no short row
  beside it). Play leads (a labelled Add to queue wrapped onto the bottom of a card stretched to the
  shelf's tallest). A suggestion the catalog stops offering returns the pane to the shelves. Search
  mark = `search_instead` + `choose_pane(Pane::Tracks)`: narrow further, save under its own name.
  *Shuffle* loads rows from a place picked off the clock's nanoseconds (`somewhere_in`; no random
  crate in the tree), then turns transport shuffle on.
- **A suggestion's art is drawn, never stored; a playlist's is the same drawing.** `views/mosaic.rs`
  is the one builder: `Mosaic` = covers drawn, two accents, a mark, a name; `Framed` = `Alone`
  (bordered, rounded all round) or `OnACard` (heads a card, foot square). Two copies once existed,
  only the suggestions' rounding tiles (playlist mosaic: square corners in a rounded frame).
  `suggestion_art` draws covers in `Suggestion::pictured_by` (one whole, or two to four as a 2×2
  mosaic, empty tiles the accent solid) over a 135° gradient between two accents of the worn palette
  via `theme::hue` (follows the theme). Accents are the reason's where it has a colour (a decade
  amber into peach, *Most played* red into peach); a genre's or artist's come from an FNV hash of
  the name (same colours every run, names differ). No cover: ground carries the reason's icon and
  the name in bold, in `theme::ink_over` the first accent. Covers via `LibraryModel::cover` at grid
  size on a card, `Drawn::OnThePage` once opened (cards fill in as decodes land). **The gradient is
  painted only where no cover is**: under the whole frame, gpui clips a child to its parent's
  rectangle, not its rounding, so square covers stood over the rounded ground, a hairline of it at
  the edge and between half-pixel tiles. Side = whole pixel, near tile the floor of half, far tile
  the rest (`tile_span`), so far tiles reach the edge on an odd side; tile and cover round on the
  one corner they stand in (`Corner::of_tile`), a single cover rounds itself. **A frame standing
  alone draws its hairline over the art, never as its own border**: `border_1` took two pixels off
  the tiles' box, right and bottom tiles overran it and the rectangular clip squared every corner
  but top-left (a single cover lost right and bottom rounding). `hairline` = absolute, rounded,
  bordered child laid last; art fills the whole side. On a card the bottom edge stays square,
  meeting the text.
- **The playlists index is a grid of covers or a list; the heading chooses.** `PlaylistsDrawn` =
  `Grid` (default) or `List`, a `kit::segmented` beside the sort icon, kept for the run. Grid has
  the albums pane's shape (same `grid_width`, `grid_columns`, `grid_row`): a card per playlist,
  mosaic at `theme::grid_cover()`, name (accent under the pointer or in play; led by a search mark
  where it fills itself), then count and when last played (length where never). The pin sits in the
  card's corner like a favourite's star (hidden until hovered unless pinned); a round accent Play
  rises in the other corner under the pointer. List = the rows it always was, each with a context
  menu beside its controls, neither answering the other's press: gpui 0.2.2 starts a click only on a
  left press, so a right press on play, next, last, shuffle, add songs or discard opens the row's
  menu and fires none. A pinned row wears a `kit::badge` beside `SEARCH` and `KEPT` (catalog already
  sorts pinned first; the pane orders nothing). `Listed::Playlists` makes either shape a reach-key
  listing like the albums grid: a page is whole grid rows, `show_row` scrolls the row holding the
  playlist, `enter` opens it. An empty index offers *New playlist* and *Import* under its sentence.
- **What pictures a playlist is read with the listing, never on the frame.** Index rows once read
  each playlist's whole entries from SQLite on the render thread, discarded every library revision
  (every counted play, every scan poll). The load now asks `Library::playlist_pictures` for every
  listed playlist and the opened one, `PICTURED_AT_MOST` (4) covers each; `LibraryModel::pictured`
  hands them out by reference count.
- **Pinned playlists stand in the sidebar under Playlists.** `Library::pinned_playlists` rides the
  same load: at most `PINNED_IN_THE_SIDEBAR` (5), most lately pinned first, unnarrowed by search.
  `RootView::pinned_rows` draws each as a short indented name under *Playlists*, opening it (open
  one text colour, in-play one accent). The index still lists every pinned playlist first; five
  bounds the sidebar, not pinning.
- **An empty opened playlist says why and offers the way out.** A list offers *Add songs* (heading's
  + track-browsing mode); a saved query matching nothing says the library holds nothing it matches
  yet and offers *Edit search*; a narrowed playlist says only that the search matched nothing in it.
  Discarding one toasts its name and `ctrl-z`, as starting one always did.
- **The sleep control draws no clock of its own.** Between repeat and volume in the bar, it opens a
  `Menu` rather than cycling (a toggle would be pressed through the choices): seven choices
  (`SLEEP_MINUTES`' five lengths, end of track, end of queue), eight while a timer runs (*Turn the
  sleep timer off* joins). Accent wash and countdown sit in one `when_some` on
  `PlayerState::sleeping`: unset leaves a moon at shuffle's and repeat's weight and nothing else;
  reading comes off the publication, a part second rounds up (just set reads `15:00`, not `14:59`).
  Deliberately not a setting, no config key: a sleep timer is a nightly decision, and a key with no
  control is what the settings rebuild went hunting.
- **The queue is reached from the playback bar, not the sidebar.** `Pane::BROWSE` is what the
  sidebar lists, `Pane::Settings` pinned under it by `justify_between`; `Pane::Queue` only by the
  bar's queue button or `ctrl-u`, a `toggle` like shuffle and repeat, accent when open. No count:
  the heading gives the length, and a figure beside the cluster's only icon with one read as a badge
  to clear. `RootView::behind_queue` remembers the pane the button covered; a second press returns
  to it.
- **The queue follows the row that started playing, only while it is the pane in front.**
  `RootView::following` is a `Following`, the `PlayingRow` last shown: the
  `PlayerState::queue_position` row, the playing track's id, the queue's `queue_stamp`, all off one
  published state. `Following::follows` is the whole decision: the row, or nothing where `self.pane`
  is not `Pane::Queue`, nothing plays, or the row has not *moved on*: another track plays, or the
  row changed inside the same queue (same track queued twice, stepped onto). A row moved only
  because rows above were dropped, dragged or put back changes the stamp and keeps the track:
  recorded, not scrolled to (following the index threw the view away from rows being edited). It
  rides the `cx.observe(&player, …)` `count_a_play` rides and lifts the *NOW PLAYING* heading to the
  pane's top: `QueueParts::opens_at` is that line, `lift_the_playing_row` scrolls to it strictly
  (gpui's plain `scroll_to_item` does nothing for a line in view), so what was heard sits above the
  fold. Opening the queue pane forgets the row last shown (so it always opens that way). A heading
  near the end could not reach the top of a list ending under it: `QueueParts::room_below` adds
  blank lines for what the pane's measured height leaves unfilled below it, and the list is not
  built until `queue_height` is measured (the album grid's rule), so the first scroll is taken
  against the room it needs. Reach keys still centre the row they land on. Holding the row keeps the
  16 ms player poll from fighting the listener: a redraw leaving the playing row where it was
  scrolls nothing (a queue scrolled away by hand stays until the track changes). A row that moved
  while another pane was in front is not recorded as shown, so the first poll after the queue
  returns lands on it (scrolling an unwatched list only yanks it later). `Following` is arithmetic
  over `Option<PlayingRow>`: `views/root.rs` tests it windowless, as `views/reorder.rs` tests
  `Step::landing`.
- **The *Reads* row reads back the search box; it claims nothing about a listing.** `listing::reads`
  = one chip per clause off `Search::reads`, in every pane's heading: under the tracks pane's, the
  playlists index's, an opened playlist's, beside the albums and artists titles (counts only). It
  says how the text was read, not what each pane did with it: `Matching::grouped` carries the whole
  grammar into albums, artists and tracks listings, `db::cuts_matching` into an opened playlist's
  rows, while the playlists index narrows on `words_of` alone (a playlist has only a name; its empty
  message says so). A lone word draws no chip. A pane with nothing left says "No albums match."
  where a search is in force, "No albums yet." over where to add a folder where none is (the branch
  blaming a search is the branch a search caused).
- **The window keeps the queue for the next run; the switch stopping it discards what was kept.**
  `RootView` holds a `Keeping` (field `resuming`) beside `Listening`, off the same `PlayerModel`
  observer, keeping what `resonate play` keeps by the same arithmetic (`audio.md`). `Stored::resume`
  is what the binary hands in, a `config.toml` key the window draws and obeys, so `LibraryModel`
  holds it beside `online` and `after_scan`. The *Resuming* card is a Library group, not Online: it
  writes to the catalog, no network. Off = `Library::forget_resumption` at once, not merely ceasing
  to write (a queue left behind came back when the setting was turned on again). Each write runs
  on a background task that first awaits the one before it (`keep_after_what_was_kept`): replacing
  the task would cancel a queue write still waiting for a thread behind the place write after it.
- **Tab, restore size and Settings category each have a memory switch.** `remember-tab`,
  `remember-window-size`, `remember-settings-category` default on and ride into `ResonateApp` via
  `Stored`. The tab is the pane `in_front` names (a scoped album is remembered as Albums); a tab
  hidden by `Tabs` is ignored on the next open. The size is the windowed restore size, written after
  `WINDOW_SIZE_SETTLE` (400 ms) so a drag edits the config once, not per compositor resize event;
  the window opens centred at it. The Settings category restores independently. Switching off
  removes the saved value, so switching on later starts from the current tab, size or category.
- **The window is handed its lookups and owns none.** `Lookups` is what `run` takes beside player,
  library and settings: `lyricists`, `fingerprinters`, `reference` (`Option<Arc<dyn Reference>>`),
  `for_the_pass` (`Consulted`: reference and fingerprinters), `scrobblers`, `signs_in`,
  `corrections`, the `online` setting (`enabled`, `contact` as saved), `bindings`, `sourcing`,
  `listens`. `ResonateApp` holds them as globals, so `LibraryModel::new` knows whether it
  `can_enrich` and the settings pane knows what to draw. A scan not cancelled runs `enrich(false)`
  as it ends where `can_enrich` (online *and* a reference) allows; the lookup's task polls at
  `SCAN_POLL` and reloads every `POLLS_PER_RELOAD` polls like the scan's, so rows and covers arrive
  as the reference answers. Online off under a run keeps the reference: it only stops `enrich`
  starting, and `has_reference` is what the card reads to say a run started offline has nothing to
  reach until the next start. The shared client hears the switch at once (`online.md`);
  `AnalysisModel::reach` stops the Analysis pane asking for a recognition while off (`recognises`
  false).
- **A file changed under a root while the window runs is followed on its own; one taken away is
  forgotten at once, not scanned for.** `resonate_library::RootsWatch` = recursive inotify watch
  over the roots (through `notify`), sorting what it hears two ways:
  - *Gone* (audio file or folder removed, or anything but a sheet renamed away): noted by path,
    handed out by `taken_away` once `GONE_QUIET_FOR` (250 ms) has passed since last heard (a tagger
    deleting and rewriting in one breath is seen standing again), never while a change under the
    roots is still settling (a rename is a path gone and changed at once; the scan it triggers
    follows the file to where it went rather than forgetting and recounting it).
  - *Changed* (audio file or sheet made, written or renamed in; a folder made or renamed in whatever
    its name holds, `Dr. Dre` as readily as `Meddle`; a sheet taken away): notes its root; `settled`
    hands it out once quiet for `ROOTS_QUIET_FOR` (2 s). An overflowed inotify queue notes every
    root.
  - A catalog inside a root cannot trigger the scan writing it (no audio or sheets there); a vault
    inside one does (its objects are audio), and the following scan steps past it.
  - The model's `watch_the_roots` looks every `ROOTS_LOOKED_AT_EVERY` (250 ms). It rebuilds the
    watch wherever `Library::roots` moved, over roots present as directories:
    `RootsWatch::taking_over` carries every root and path the old watch heard and had not handed
    out, with when each was heard (a drive coming or going costs no other root a change still
    settling); a watch that cannot be laid leaves the old one standing and the roots untried,
    retried next look. A returning root is scanned. It hands what is gone to
    `Library::forget_the_gone` (takes the `Walk` guard, deletes rows at each path or under it whose
    file is missing, sparing a vaulted and a delivered row as the scan's prune does, and rows on an
    absent volume; sweeps what that orphaned) and reloads where it forgot anything. Where a root has
    settled and nothing holds the work slot it runs the ordinary incremental scan of it,
    `Prompted::OnItsOwn`; the first look after the window opens runs one over every present root
    (for what changed while closed).
  - A non-UTF-8 path names nothing storable: passed over, not failing its batch. A forget refused
    because a scan or import holds the guard (`Error::AlreadyWalking`), or failed by the store, is
    kept and retried next look, so a file deleted mid-scan leaves the list when the scan ends, not
    after another quiet period and whole-root scan. (This replaced waiting for quiet then rescanning
    the whole root per deletion: files deleted one by one kept pushing the scan back, and a large
    root took its whole walk to drop one row.)
  - **A steady writer cannot defer the scan for ever**: `settled` also hands a root out once its
    first change has waited `DEFERRED_AT_MOST` (5 minutes) however lately the last was heard
    (`a_folder_that_never_goes_quiet_is_handed_out_once_it_has_been_deferred_long_enough`).
  - A watch that cannot be made (inotify out of watches) is a warning; one that cannot cover a root
    (`RootsWatch::leaves_a_root_uncovered`) is laid again every `UNCOVERED_ROOTS_TRIED_AGAIN_AFTER`
    (5 minutes); a root it never covers waits for a scan by hand. Proved past the limit itself
    (`a_root_past_the_inotify_limit_is_said_uncovered_and_the_root_watched_before_it_still_hears`
    reruns its binary under `unshare --user` with `max_inotify_watches` lowered to eight: the
    limit is the user namespace's, so the desktop's own watches are never spent).

- **Closed-window changes are caught up on open; a missing root is watched once it appears.** A
  watch hears only while it stands (files added between runs waited for a manual scan). First look
  that has read the roots runs one incremental scan of every root, `Prompted::OnItsOwn`, named no
  roots so an unmounted one is skipped by `is_there` instead of failing the scan
  (`RootNotADirectory`) and holding back the rest. Waits for the work slot like any owed root;
  an unfinished lookup does not hold it; dropped if no root is there. `Watching` watches only
  roots that are folders now, keeps the rest as `absent`; one turning up later (drive mounted
  after open) rebuilds the watch and is owed its own incremental scan (was read once, never
  watched).
- **A browse pane holds a prefix of the listing, but is as long as the whole match.**
  `LibraryModel::reach` = rows the four browse lists are read to, `PAGE` (2 000) to start. Each
  list's `uniform_list` counts the catalog's match (`albums_listed`, `artists_listed`,
  `listed_rows`: the larger of what is held and `albums_counted`/`artists_counted`/`listed().rows`),
  so the scrollbar, `End`, `ctrl-a` and the page keys reach the last row of a 50 000-track library;
  a row not yet read draws as a blank row of its height (a grid row with fewer cells). The
  `uniform_list` processors (only place knowing how far a list is drawn) call `reach_further`: once
  a list has drawn within `LOOK_AHEAD` (200) of what it holds *and* holds all it asked, `reach`
  grows to cover what was drawn (`reach_covering`: a thumb dragged to the end reads the rest in one
  read, not a page per frame); `held < reach` = re-entrancy guard. **A page read reads only what
  lies past the rows held** (`grown`, `Wanted::ThePage`): `Asked::held` names each list's length
  when asked; each list is read from one row before its end (`offset` = held less one) up to
  `reach`. `grew` adds the read only where its first row is the row held last (`Grew::Renewed`);
  where the list's length or its last row moved under the read (a scan wrote between), it answers
  `Grew::Moved` and the model reads the lists whole once (`page_moved` zeroes `Asked::held`), so a
  second offset read can never duplicate or skip a row. Scrolling to the end of a large library was
  quadratic before (every page re-read the whole prefix). `LOOK_AHEAD < PAGE` is a `const`
  assertion. `set_query` and `select` reset to one page (rows under the old window are not a new
  query's). A row pressed or reached past what is held plays through `play_the_listing_from`'s
  whole read, starting at its place in it
  (`a_page_read_past_what_is_held_is_added_to_it_only_where_it_carries_on_from_the_last_row`).
- **A count is what the query matches, not what the window holds.** `Library::albums_counted`,
  `artists_counted`, `measured` answer over the whole match; the first two share scoping with the
  listings via `narrowed_onto` (count and listing cannot disagree on the search's meaning).
  `Measured` = rows, total length, lossless count (from `tracks.codec IN (…)` built off
  `Codec::ALL` filtered by `is_lossless`: SQL and Rust cannot drift). Sidebar's three figures and
  the tracks heading summary read these, not `Vec::len` (said 2 000 for any larger library).
  `measured` honours a limit it is given (a saved query's cap is part of what it *is*); the
  window passes `limit: None`.
- **A gesture over "every track listed here" reads the whole listing first.**
  `RootView::with_everything_listed` reads the unlimited `TrackQuery` from `listing_whole` on the
  background executor (bg; a task, not a blocking read: may be the whole library), hands rows to
  a closure; tracks-heading *Play*/*Shuffle* act on all the search matches, not the loaded window.
  Whole-listing queue/add-to-playlist actions are not in that heading; track rows still expose
  queue controls on hover.
- **A scope belongs to the tracks pane; the sidebar counts the library.** `Selection` narrows one
  listing: `browsed` reads the albums, artists, tracks the search allows, plus a *second*,
  narrower track listing only where an album or artist is scoped. `LibraryModel::tracks` = the
  first; `LibraryModel::listing` = what the tracks pane, heading, summary, *Play*, *Shuffle* draw
  from. Sidebar counts hold however deep the listener goes (one listing narrowed in place cut
  the albums pane and count to the opened artist's and the tracks count to the opened album's,
  unexplained). The second read is cheap (an album's dozen rows, not two thousand), hence the
  whole listing is the one always read.
- **A scoped heading names its artist on its own line; its cover magnifies.** Album hero draws
  the owner between title and summary, not folded into the summary's first field (a name run
  together with *1973 · 10 tracks · 43:12* is unpressable at its own width), via
  `RootView::opens` like the playback bar's (gesture and hint written once). Scoped cover has
  `COVER_HINT` and `RootView::magnify`: the album being read magnifies like the one playing. A
  portrait does not (`Magnified` is an album or a file).
- **Album rows = catalog rows merged with the release's missing ones.** `LibraryModel::rows` =
  `Arc<[ListedRow]>` from `album_rows` when the selection is an album: `ListedRow::Held` indexes
  the tracks listing, `Missing` the release rows with `track` `None`; `Disc` heads each disc's
  run (two or more discs, seat order only); `NotHeld`/`Found` sit below an artist page's rows.
  Sorted by `(disc, position)`: a held track at its paired release row's place, else its own
  disc and number, no number last. The tracks pane's `uniform_list` counts `rows` in an album,
  `tracks` elsewhere. Seat order while the pane's sort is the album's own (`Relevance`,
  `AlbumThenTrack`), reversed where it reads backwards; any other heading-icon sort draws held
  rows in that sort, then what the album lacks (`arranged`). **What plays is what is drawn:**
  click/Enter go through `LibraryModel::played_from` (queues `Held` rows in `rows` order);
  *Play*, *Play next*, *Add to queue*, *Add to playlist* put the whole listing through
  `AsDrawn::ordered` (same arrangement, same release rows): the pressed row starts, what follows
  is the rows under it on screen.
  **A missing row on an album page is a download press.** `Beside::AnAlbum` makes an unheld
  release row clickable when it has no want; title/artist/length click asks `LibraryModel::want`;
  marks and dismissals stop propagation. Where the heading counts missing tracks, *Get the rest* =
  `LibraryModel::want_missing_tracks`: every unresolved release row wanted in one library
  transaction, one provider poll for the batch. `unheld_row` draws faint in the same eight cells
  (number, title, artist, length off an `Unheld`, `From` a `HeldReleaseTrack` here, `From` a
  `MissingTrack` in the Missing pane); `controls_place_of` takes the held row's controls width
  (`TRACK_CONTROLS` 3, `TRACK_ADD_CONTROLS` 2 while adding songs; Missing pane `ROW_CONTROLS` 4)
  so cells align with held rows and header. One mark: `Icon::Want` sends `LibraryModel::want` (bg,
  then `wants_landed`: `list_as_downloads`, `fetch_or_say_nobody_can`, reread) or `Icon::Wanted`
  sends `unwant` (via `edit`, `Change::Unwant`); `Loaded::wanted` maps `ReleaseTrackId` to
  `WantId`. **A want says how it is going where pressed:** `LibraryModel` keeps each want's
  `WantStanding` from the shelves; `fetching_want` reads a `Fetching` (*Queued*, *Downloading*
  while the poll asks for that want, *Retrying*, *Gave up*, *Downloaded*); the format cell says
  it in `fetching_colour` (a filled heart is never the only answer). `want` and
  `want_missing_tracks` also list each wanted row with a recording in `Downloads` as a `Found`
  built from the album page's release rows (`list_as_downloads`), shown by sidebar and panel like
  a found song's attempts; both start the poll via `fetch_or_say_nobody_can` (toast *No provider
  is set up* where `has_a_source` is false, as a found song's caption does). Grid caption and
  scoped heading no longer count what an album is short of: Missing pane, inline `unheld_row`s,
  sidebar figure and artist heading's *N releases not held* each say it once where it is the
  subject. The pressing is not drawn there. Info mark at row end under the text, only where
  `record_of` has something to say, opens a card anchored at the press: Released, Format, Label,
  Catalogue number, Barcode, Country, Kind, disambiguation, every service link. With a release
  group and a reachable reference, *Other pressings* asks `Reference::release_group` on bg
  (`LibraryModel::ask_for_pressings`, held in `pressings`), lists the group's releases in release
  order, first `PRESSINGS_SHOWN` (12), each date, country, track count, the one in use badged *IN
  USE*; pressing another = `LibraryModel::take_pressing` (`Library::take_pressing`, closes the
  card). **A track is placed on a release from its own menu:** *Place on a release…* (reference
  reachable) reads the track's recording via `LibraryModel::releases_it_could_sit_on`, opens a
  second menu where the first stood, releases listed as a found song's (title, year, kind;
  `RELEASES_OFFERED` 10); press = `LibraryModel::place_on`; an unidentified track says so as a
  toast. **An album nothing matched has the card too** (reference reachable): mark drawn however
  little `record_of` says; card offers *Find the record* instead of *Other pressings*;
  `LibraryModel::ask_for_releases` asks `Reference::find_release` by words (title, artist, any
  barcode/catalogue number from tags), lists results like a group's pressings each with its
  credit, so a press is the same `take_pressing` (the listener settles what a lookup could not).
  Where matched, the card ends in *Not this record*: two presses (second *Press again to forget
  the match*; arming held on `OpenedRecord::Album` so any way the card closes lowers it) =
  `LibraryModel::forget_the_match`, toasted. Escape closes the card after a magnified cover,
  before a toast; a second press on the mark and leaving the album (`set_pane`) close it. Scrim
  occludes, as a menu's. Artist hero draws `profile_line` and `heard_on`; genres open from the
  info mark (all, as `kit::tag` pills; mark only where there is one). At the end of its actions,
  where `ArtistDetail::releases_unheld` > 0, a `Tone::Ghost` button *N releases not held* (plus
  *, M more unread* where `releases_unread` > 0) under `UNHELD_HINT` opens `Pane::Missing`; with
  unread releases and `can_enrich`, *Read the rest* = `LibraryModel::read_the_rest_of`
  (`library.md`). **An artist's services are capped at `SERVICES_SHOWN` (4)** (an artist can name
  nineteen: every streaming service, both encyclopaedias, four social networks): the heading
  draws one line of words, not a cloud of mono figures; a release's card and the genre card are
  uncapped (room). **Each service is the link it was named from.** `service_names` keeps the
  first URL per service beside its `Service::title` (*Apple Music*, *SoundCloud*, not the
  lowercase key), omitting `Service::Other`. `heard_on` and the record card draw one pressable
  name per service, lit under the pointer as an `opens` is, *Open on …*, handing that exact URL
  to `cx.open_url` (desktop browser); press stops propagation, ignores a right click, like every
  heading link.
- **The artists pane is a grid or a list; the heading chooses.** `resonate_core::ArtistsDrawn` =
  `Grid` (default) or `List` (rows, small portrait beside each name). `Grid` = albums pane's
  shape with portraits for sleeves: `artist_grid` measures the same `grid_width`, reads
  `grid_columns`, lays rows of `artist_cell`s at `theme::grid_cover()`, each a round
  `portrait_frame` read at `Portrayed::InAGrid` (`kit::avatar_at` at that size where no portrait
  is held, initial scaled with it) over name and counts, favoured, pressed, menued as a row.
  Choice = `kit::segmented` *List*/*Grid* beside the sort icon, persisted:
  `RootView::draw_the_artists_as` writes `Setting::ArtistsDrawn` (`artists-drawn`) and
  `ResonateApp::artists_drawn`, which the next run opens on. Reach keys serve both: in the grid a
  page is whole rows of cells, `show_row` scrolls the grid row holding the artist, a reached cell
  wears the album's `reached_ring`.
- **An artist's page is its albums or its tracks, one at a time, from two tabs.** (Was a
  horizontally scrolling strip of small covers over the whole track listing: three scrolling
  regions, albums as thumbnails, the artist's name on every row.) `artist_tabs` = `kit::segmented`
  *Albums*/*Tracks* with counts, right of the action row (a band under the hero would be the gap
  between title and listing); `RootView::artist_shows` = chosen `ArtistShows`, kept for the run,
  reset to `Records` whenever `RootView::opened` opens an artist. Heading keeps its bottom
  padding so the rule under the title does not sit on the buttons. *Albums* = `artist_records`:
  every album the artist owns or plays on, a wrapping grid of `album_cell_captioned` at
  `theme::grid_cover()`, scrolling under an id keyed by the artist (the next artist opens at its
  top); `Caption::Beside`: year and track count, owner only where somebody else's (what an album
  the artist merely plays on must say). *Tracks* = ordinary listing under the ordinary header,
  the only tab with the sort icon (nothing to order under the other). `ArtistShows::within`
  answers *Tracks* for an artist with no album, held or not; then no tabs. *Play*, *Add to
  queue*, *Play next*, *Add to playlist* read the whole listing either way (they act on the
  artist, not the tab).
- **What an artist's discography holds that the library does not is on the artist's page,
  greyed.** Rides in the load beside the albums (`Library::albums_not_held_by`,
  `songs_not_held_by`; `library.md`), Missing tab shown or not. Selecting the artist also starts
  `LibraryModel::learn_the_songs_of` where Online is on, reading songs of unheld releases the
  lookup pass has not reached. The walk reports through a `Learning` (`PageLearning`, unbounded
  channel) as each group lands; the window re-reads the page per drained batch while that artist
  is selected, so songs arrive as read (most of an artist off one browse, `library.md`), not at
  the end or whenever the pass reaches the artist. Opening another artist replaces `_learning`,
  dropping the receiver; the walk sees `abandoned` before its next request (a left-behind artist
  asks MusicBrainz (MB) nothing more). *Albums* follows the held cells with `not_held_heading`,
  *Not in your library · N releases* (full-width line breaking the grid), then an
  `album_not_held_cell` per release: pressing's front via `released_cover_with_group` at
  `UNHELD_COVER` (0.45), read from `Library::unheld_cover` (filled by the lookup pass,
  `library.md`) before the archive is asked live, drawn offline from what is kept; release-group
  id = fallback and the lookup where no pressing is known; `unheld_cover` at the grid's side where
  neither answers; title in `theme::faint()`, kind and year under. Pressing one =
  `LibraryModel::land_album_not_held`: `Library::open_album_uncovered` on bg (release read whole,
  landed as an album with release rows and no want, cover asked after), opens the album, where
  each missing song is pressed to be wanted as a short album's are; `want_album` still serves a
  link to an album not held. `fetching_album` reads the album's songs from `Downloads` (active
  attempt where any, else first still underway, else first not downloaded); caption says it in
  `fetching_colour`. An album landed with nothing on disk yet stays in the grid. *Tracks* follows
  the held rows with `ListedRow::NotHeld`, *Not in your library · N songs · press one to download
  it*, and a found row per song (the `found_row` a search draws; press = `want_found`);
  `restate_the_listing` builds them via `beyond_the_listing`, which answers nothing until the
  listing is whole (a paged listing never draws later pages under the section). A page query
  narrows them via `still_answering`.
- **Missing pane = what the catalog knows it is short of, headed by run, one half at a time.**
  `Pane::Missing` sits under `Section::Collection` beside the playlists; sidebar count
  `Missing::tracks`, hidden at zero (a library the reference never described shows no figure).
  Two lists, two tabs as on an artist's page: `missing_tabs` = `kit::segmented`
  *Tracks*/*Releases* with counts; `RootView::missing_shows` = chosen `MissingShows`, kept for the
  run. `MissingShows::within` hands over to the half holding something where the chosen one is
  empty; tabs only where both hold; the artist heading's *N releases not held* opens on
  *Releases*. (Was one list, where an artist's heading after the last album's rows read like
  another album's.) Each tab = one `uniform_list` over `LibraryModel::missing_track_rows` or
  `unheld_release_rows` (`Arc<[MissingRow]>` of `Album`, `Disc`, `Track`, or `Artist`, `Release`;
  indices built by `models::missing_track_rows`, `unheld_release_rows`): a heading wherever the
  key changes, a row per entry
  (`each_run_of_an_albums_missing_tracks_is_headed_by_the_album_once`,
  `each_run_of_an_artists_unheld_releases_is_headed_by_the_artist_once`). Artists' half =
  `headed_by_run` over one key; albums' half = `headed_by_album_and_disc` over album *and* disc (a
  set's missing rows number from one again under each disc; one heading over the lot would read as
  one album with two track ones). Disc heading only where the album's own run spans more than one
  disc (`headed_by_disc`'s rule; `an_album_missing_rows_from_two_discs_is_headed_by_each_of_them`):
  eyebrow *DISC N* under the title column, no more (no `HeldMedium` to name a format or title
  with). **A run is a card drawn a row at a time (every `uniform_list` row is one height).**
  `Place::of` = a row's place: heading `Head`, row before the next heading or the end `Last`,
  between `Within`; `in_a_card` draws that slice of a `kit::section`-like card: heading on
  `raised` with top edge and rounded top corners, rows on `surface` with a hairline under each,
  last closing with rounded bottom corners. The gap between cards cannot be a margin, so it comes
  out of the bordering rows: heading and last row are each `HALF_BETWEEN_CARDS` (6) shorter than
  their row, heading at its row's foot, last at its head
  (`a_run_is_one_card_opened_by_its_heading_and_closed_by_its_last_row`). (Loose rows under a
  heading band read as text floating with nothing holding it.) `run_band` = heading content:
  album cover or artist portrait (`kit::avatar` where none) in the number column, name via
  `opens` in semibold text colour, owner beside it muted, the run's count (*7 missing*, *11
  releases*) ending where lengths end. Track row = `unheld_row` under `Beside::ARun`, dropping
  format and plays cells (no header for them); release row = `release_row`: title muted, kind a
  `kit::badge`, first release year in the length column, no press (a release the catalog holds
  nothing of names nothing it can show). Subtitle speaks for the tab in front (*8 tracks missing
  from 2 albums*, *46 releases by 8 artists not held*). `Loaded` reads `missing_tracks` and
  `unheld_releases` under `MISSING_AT_MOST` (5 000) and `missing_counted` on every load: tracks
  and releases the subtitle and tabs count are the catalog's; albums and artists beside them are
  the headings the list drew. **A list cut short of its count says so:** where the tab lists
  fewer rows than the catalog counts, `listed_short` writes *5000 of 7214 tracks listed; type to
  narrow to the rest* under the subtitle (a count above 5 000 rows read as a list that ended).
  **A row is dismissed with its ✕.** Track row: `Icon::Close` beside its want mark under
  `Beside::ARun` alone (not an album page's missing row or a search's), sending
  `LibraryModel::dismiss_missing`; release row: one sending `dismiss_release`; both via `edit`
  as a want is; heading's *Bring back N dismissed*, drawn while `Library::dismissed` counts any,
  = `bring_back_dismissed`, toasted (`library.md`: what a dismissal is). Empty: `kit::empty`
  under `Icon::Missing` saying nothing is missing (*Nothing missing matches.* while narrowed);
  where the build `can_enrich` a second sentence says where the answer would come from.
- **A song asked for is followed in the sidebar, never told in a toast.** `downloads.rs` is the
  whole of it: `Downloads` holds a `Download` per found song pressed (the `Found`, `WantId` once
  landed, when queued, a `Fetching`); `LibraryModel::want_found` writes it instead of toasting:
  `Landing` while the release lands, then `Queued` (`NoProvider` where `Sourcing::providers`
  registers none, `Unwanted` where landing failed). `Download::fetching_while` reads a queued
  download as `Downloading` while the poll's `Polling` (from `PollProgress`) names its want;
  `Downloads::followed` reads each shelves load's `WantStanding`s: delivered (`held`/`offered`) =
  `Downloaded`; tried since queued with nothing offered = `Retrying { tries, at }`; given up =
  `GaveUp`. **A download says two things while underway: *Looking it up…* and *Downloading…*.**
  `Landing`, `Queued`, `Retrying`, `Unreached` and a `Downloading { attempt }` with nothing yet
  received all read *Looking it up…* (a want asked again after a miss, or one a provider did not
  answer, never shows an attempt count, clock time or *No match* between polls: several attempts
  read as a janky alternation). `downloads::saying_while` reads *Downloading…* once
  `Fetched::is_arriving` (`LibraryModel::fetched`, off `PollProgress::asking_provider` and
  `received`), redrawn each `SCAN_POLL` the poll's task notifies, in the panel and on the song's
  own row (found song `told_found`, album's missing row `told_want`: same words, not *Looking it
  up…* to the end). Sidebar `summed_up` takes whether any download is arriving, says one of the
  two with *· N left* where more than one song is underway; none underway: *N downloaded · N not
  downloaded* (*Downloads* when empty); attempt counts are each row's to say
  (`a_download_says_it_is_looking_the_song_up_until_bytes_arrive_and_downloading_after`,
  `the_sidebar_says_looking_it_up_or_downloading_and_how_many_songs_are_left`). Only an ending
  speaks otherwise: *Downloaded*, *Not found* (`GaveUp`), *No provider is set up*, *Couldn't add
  it*. A poll ending with a provider refusing or late (`PollStats::refused`/`late`) sets
  `LibraryModel::providers_unheard`; while set, a queued download, or a retrying one whose clock
  time has passed, reads `Unreached { attempt }` (such a want is never stamped, `providers.md`);
  a poll hearing every provider clears it, a cancelled one leaves it (`Fetching::while_polling`
  is the one reading, shared by song list and album cells). All count as underway. A want the
  load does not hold is left as it stood (a stale load is no evidence it went). **Standings are
  re-read while the poll runs:** `Followed` weighs the poll's `PollStats` each `SCAN_POLL` and,
  where they moved and `POLLS_PER_REREAD_WHILE_ASKING` (5) ticks have passed since the last
  read, reads the shelves (whole page where a delivery was kept, a new row being drawn): a landed
  song reads *Downloaded* while the poll goes on (was *Queued* until an album's last song was
  asked; `a_poll_that_moved_is_read_again_while_it_runs_and_a_landing_rereads_everything`).
  **The list outlives the window.** Catalog wants are the downloads: first shelves load hands
  `Downloads::restored` an `Unfinished` per want with a recording (newest last, as pressed);
  every one still underway (not delivered, not given up) is listed again with its standing's
  state (`a_song_still_wanted_is_listed_among_the_downloads_when_the_window_opens_again`,
  `what_was_still_underway_comes_back_after_a_restart_and_nothing_finished_does`). Sidebar draws
  `RootView::download_status` above the enrichment line while the list holds anything:
  `Icon::Download` in the accent while any is underway, label `downloads::summed_up`. A press
  opens `downloads_over_the_app`, a panel floating `DOWNLOADS_PANEL_GAP` (8) beside the sidebar
  and above the playback bar (a short window's sidebar had room for one song and a half):
  `theme::downloads_width` (a state on one line), list at most `theme::downloads_height`, scrolled
  past. A press outside closes it; the sidebar row's own click right after such a press is
  ignored for `DOWNLOADS_TOGGLE_PRESS` (600 ms), so pressing the row to hide it does not reopen
  it. Each song: title, artist, state in `browser::fetching_colour` (accent downloading, `done`
  downloaded, `failure` given up/unprovided/unwanted, `muted` landing/queued/retrying), three
  lines of one clipped column, each `truncate().ends_in_an_ellipsis()`, beside a `flex_none`
  group of marks (a long state like *Attempt 2 of 5 · asking …* ends in an ellipsis instead of
  running under the marks, as when it sat `flex_none` beside the artist). The text is a press
  opening the album the want is on (`LibraryModel::downloaded_album`, off its standing) and
  closing the panel; a downloaded song has `Icon::Play` reading the row it landed as
  (`LibraryModel::downloaded_track`) and playing it; `Icon::Redo` where `can_be_asked_again` (a
  retry tries at once; a given-up song restarts from the first try); ✕ on a finished one; *Clear
  finished* and a close mark in the heading. A song with `Fetching::can_be_cancelled` (queued,
  downloading, unreached, retrying; not landing: no want yet) has `Icon::Stop` beside the redo:
  `LibraryModel::cancel_download` takes it off the list, withdraws its want (`Library::unwant`)
  and, where the poll is asking for that very want, cancels the poll (other wants stay due, asked
  again by the next). A second press on the row, the close mark or escape (after a menu, before a
  toast) puts it away; not modal, holds no key back. Asking again = `want_found` once more,
  making the want due at once in the catalog (`library.md`) so the ordinary poll asks whatever the
  list remembers (was: relied on the list holding the earlier failure, so a song asked again
  after *Clear finished* or a restart sat *Queued* six hours). A self-started poll joining while
  the list holds anything tells no toast. The found row says the same where it stands: format
  column draws the `Fetching` in its colour instead of the release title, want mark greyed with
  the state as hint, row a press only where nothing is asked for yet or `can_be_asked_again` (the
  song stays in results showing progress instead of vanishing when the search is asked again).
  Section heading: *Press a song to download it*.
  `pressing_an_ncs_song_found_on_musicbrainz_wants_it_and_asks_the_providers_for_it` asks for a
  song a shop does not have, waits for its first retry, opens the list, holds no toast up;
  `downloads.rs` tests hold the states.
- **A song in the queue is taken out from its menu wherever listed.** A track row's menu (tracks
  pane, search, album or artist page, favourites) offers *Take out of the queue* right under the
  queue entries while `RootView::is_in_the_queue` finds a queue row with the same location and
  span; press = `take_out_of_the_queue`, dropping every such row from the last up through
  `drop_rows` (put-back toast as in the queue pane, whose rows keep ✕, menu entry, delete key)
  (`a_queued_track_is_taken_out_of_the_queue_from_its_menu_wherever_it_is_listed`).
- **A song is deleted from disk only through a dialogue saying it cannot be undone.** Track row's
  menu offers *Delete from disk…* (`Icon::Delete`) = `RootView::ask_to_delete` with a `Deleting`
  (track, title, artist, whether a cut). `deletion_sheet` = card over the scrim as the Listen
  sheet (`theme::confirm_width`): names the song, says the file goes from disk with the plays,
  that a cut takes its whole file and every cut from it, and that it cannot be undone. *Cancel*
  (`Tone::Outlined`), escape, press outside keep the song; only *Delete* (`Tone::Destructive`:
  failure colour on the raised fill, the fourth tone, for a gesture losing something for good)
  calls `LibraryModel::delete_track` = `Library::delete_tracks` on the edit chain, toasting
  *Deleted “…”* or trouble where the file stayed. No key confirms, `enter` included. While open
  it `something_stands_over_the_pane`
  (`the_delete_sheet_holds_the_keys_back_from_the_queue_behind`);
  `deleting_a_track_asks_first_and_takes_its_file_only_once_confirmed` presses *Cancel* then
  *Delete*.
- **A search is a page of its own; what the library lacks is never buried under what it holds.**
  `views/search.rs` is the whole of it. `RootView::search_in_front` answers a `SearchShows`
  wherever the box holds words, the pane in front is Albums, Artists or Tracks, nothing is
  scoped, no songs are being added to a playlist; `content` then draws `search_pane` instead of
  the pane. Heading (`search_heading`): *SEARCH* over the words in quotes, a summary (*12 songs ·
  3 albums · 1 artist in your library · 18 songs not in it*), actions (*Search the words as typed*
  beside a *Read as “…” by …* line where the search was reinterpreted, *Try again*, *Sung in*,
  *Save this search*, sort, *Shuffle*, *Play*), *Reads* chips, tabs each count in a pill: *Top
  results*, *Songs*, *Albums*, *Artists*. **A tab counts the library's and what was found beyond
  it together**, *…* after the figure while MB is asked; the summary says each half (*12 songs ·
  3 albums in your library · 18 songs · 4 albums · 2 artists not in it*); *Try again* stands in
  the actions where MB could not be reached. *Songs*, *Albums*, *Artists* are the three panes
  themselves (`tracks`, `albums`, `artists` take `search_heading` instead of their own): choosing
  one is `show_in_the_search`, setting the pane under it and keeping every reach, sort and scroll
  it had; a sidebar press on one of the three while searching is that tab
  (`SearchShows::in_place_of`). *Top results* (`top_results`) = one scrolling page: matching
  artists as a strip of portraits at `ARTIST_AT_THE_TOP` (72 px), first `SONGS_AT_THE_TOP` (5)
  songs, first `FOUND_AT_THE_TOP` (6) songs not in the library with what MB is doing beside the
  heading, albums as a strip, then *Albums not in your library*; each section headed by its name
  (`Section`) and, where more matched than shown, *See all N*, opening its tab (songs not in the
  library open *Songs*). **A tab lists what was found beyond the library under what the library
  holds, never in a tab of its own.** *Songs* = one list: `LibraryModel::rows` holds held rows, a
  `ListedRow::NotHeld` heading (*Not in your library · N songs · press one to download it*) and a
  `ListedRow::Found` row per song, once the whole held listing is loaded (`beyond_the_listing`,
  as an artist's page lists songs of releases not held); enter on a found row = `want_found`
  (`LibraryModel::found_at`). *Albums*/*Artists* keep their grid or list and end in a strip,
  *Albums not in your library* (`album_found_cell`, `AlbumFound`) and *Artists not in your
  library* (`artist_found_cell`), `flex_none` under the pane so held rows take what is left; with
  nothing held the strip stands alone; with nothing at all matched the pane says *Asking
  MusicBrainz…* or *MusicBrainz could not be reached* (`nothing_beyond`). A found album's cell =
  an artist's page's release-not-held cell (`unheld_album_cell`); press `land_album_not_held`
  opens the album as there. **An artist the search names that the catalog does not hold is a cell
  of its own.** `LibraryModel::artists_found` = the `ArtistFound`s `show_answer` weighed beside
  the songs (`Library::unheld_artists_among`, off the credits MB answered with: no request of its
  own), read only while `found_for` is the query; *Artists not in your library* = strip of
  `artist_found_cell`s under the *Artists* tab's held artists and between artists and songs of
  *Top results*; either page is empty only where this is too. Press =
  `LibraryModel::land_artist_found`: `Library::open_artist_found` on bg, opens the artist, its
  releases listed as a held artist's (`library.md`). Opening takes a request or two, so a pressed
  cell says so: while its group or id is in `albums_opening`/`artists_opening`
  (`is_opening_album`, `is_opening_artist`) a found album reads *Opening…* in the accent where its
  caption was, a found artist carries *Opening…* under its name; a second press is the same press.
  **A song not held names its artist as a link too.**
  `unheld_row` draws a `Found`'s artist via `performed_by`: `Performer::Held` opens that artist
  as a held row's name does; `Performer::Elsewhere` is pressed into `open_artist_found` (the one
  land-then-open an artist cell and a followed artist link share); no performer keeps a plain
  name. The press stops at the name (never wants the song); the rest of the row still does
  (`pressing_the_artist_of_a_song_found_on_musicbrainz_opens_the_artist_and_wants_nothing`). A
  link pasted while another is being followed is followed too, each detached with its own notice.
  A search always begins on *Top results*, read where the box goes from empty to words
  (`RootView::open_the_search`): from Albums, Artists or Tracks the pane stays and its page is
  replaced; from any other pane (Queue, Playlists, Statistics, Settings) the window goes to
  Tracks showing everything (the box lives in every pane's header; a search drawing nothing there
  looked like text typed at the top) (`a_search_begun_on_any_pane_opens_the_top_results`). (Was:
  the tracks pane listing every held row and, once paged in, the found songs; an artist with a
  hundred songs held put found ones a hundred rows down, out of sight.) So under *All tracks*
  `LibraryModel::rows` is empty until a search finds songs (one row per track, what `played_from`
  and `listed_rows` read) and `LibraryModel::elsewhere` answers the MB half alone as a `Beyond`:
  `Elsewhere(n)`, `Refining(n)` while asked again, `Asking`, `Unreached`, via
  `elsewhere_standing`. Keyboard follows: `reachable` reaches the top songs under *Top results*
  (or the found rows where none is held) and the *Songs* tab's rows as any tracks listing's.
  `songs_not_in_the_library_stand_on_the_first_page_however_many_it_holds` scans forty held songs,
  asserts the found row is drawn inside the window on *Top results* and follows the forty in the
  *Songs* tab's rows;
  `an_album_found_elsewhere_stands_under_the_albums_held_and_in_the_top_results` holds the albums.
  A search does not list rows a held album is short of (they stay in the Missing pane and the
  artist's page), only songs of releases not held at all (`songs_kept_for`). A found song's
  `unheld_row` has a `Beside::ASearch` cover column for its release's front, `Sleeve::Released`,
  which `LibraryModel::released_cover` asks the reference for on bg while Online is on:
  `FETCHES_AT_ONCE` (6) at a time, decoded on the `Drawer`, held under the release's id in
  `released_covers` (`RELEASED_COVERS_HELD` 512: more than a discography draws, else the grid
  evicts what it is still asking for) for the run; a release the archive holds nothing for is held
  as nothing, not asked again. Matched runs lit, the release in the format column; want mark =
  `want_mark` over an `Asks`: a catalog row wants its `ReleaseTrackId` as ever; a found song calls
  `LibraryModel::want_found`, landing its release and wanting the row on bg, mark greyed
  meanwhile, then asking the providers and MB again, so the song appears among held tracks after
  delivery and leaves the remote results: `restate_what_was_found` passes over a found song whose
  download's want holds a track the held listing draws (`is_listed_as_held`), so it is listed
  once, as the held track, the moment the catalog's next read carries it
  (`a_found_song_that_landed_is_listed_once_as_the_held_track`; re-weighing via `unheld_among`
  would drop it the moment it was wanted, its release landing first). **A found song's whole row
  is that press:** `found_row` lays the `unheld_row` under an id of its recording with pointer,
  hover wash and `FETCH_FOUND_HINT`: pressing anywhere wants the song and sends the providers for
  it; the mark's own press inside it finds the recording already `wanting`, does nothing twice;
  while asked for, the row takes no press
  (`pressing_an_ncs_song_found_on_musicbrainz_wants_it_and_asks_the_providers_for_it`, opening the
  window via `Driven::reaching`, a `Reaching` naming the reference and providers). What follows is
  the sidebar's downloads list. `wanting` holds a task per recording, not one for the lot
  (wanting a second song before the first landed must not drop the first's lookup and leave its
  mark grey). **A search reaches MB as seldom as it can and never leaves the listener looking at
  nothing.** MB answers one request a second, so turns go only to what is still being typed:
  - *Asked once per spelling.* An answer is kept in `answers`, a `Recent` of `ANSWERS_HELD` (128)
    keyed by `songs_asked` (the words as sent; case, spacing and grammar the ask ignores share
    one), holding the reference's matches, not the `Found`s (`Library::unheld_among` weighs those
    again on bg each time one is shown, so a song that landed since leaves the list). Typing back
    to a remembered search shows it at once, no settle, no request
    (`words_searched_again_are_answered_from_memory_rather_than_asked_twice`).
  - *One ask in flight, the latest text winning.* `ask_elsewhere_after` waits
    `ASKED_ELSEWHERE_AFTER` (450 ms) behind the keystroke, only where Online is on, a reference
    is there, the search is not scoped to an album or artist (their page answers from what it
    holds: `a_search_inside_an_artist_asks_musicbrainz_nothing`; leaving the scope asks for what
    the box holds) and `songs_asked` names words worth it; then `reach_out`. An ask has two
    halves, songs then albums: `songs_reached` draws songs at once as `Shown::InPart` (`asking`
    still standing so the summary says more is coming), then `find_albums` is sent and
    `albums_reached` draws the whole. While a chain is out (`reaching_out`) another is not queued:
    the text is `owed`; a chain whose text was typed past after its songs drops its albums and
    asks the text still in the box next (pauses mid-word cost a turn or two, not one per pause;
    an answer outrun by the box is never drawn over what it now says:
    `a_search_typed_past_does_not_ask_musicbrainz_for_its_albums`). Only a whole answer is
    remembered; a refused albums half leaves the songs drawn, is a debug line, is asked again next
    time (`songs_found_stand_where_musicbrainz_refused_the_albums_which_are_asked_for_again`).
    `found_in_part` keeps a half-drawn answer from passing as one needing no ask. Enter in the
    search box (`ask_elsewhere_now`) skips the settle.
  - *Narrowed while asking.* From keystroke until the answer lands, `found` stands in as
    `narrowed` (songs the last answer found that `still_answering` the new words) under
    `Beyond::Refining`: summary *N songs not in it so far, asking MusicBrainz for the rest…*, tab
    *N…*; only where none still answers does `Beyond::Asking` stand alone, *Asking MusicBrainz…*
    where the found songs would be
    (`songs_found_for_fewer_words_stay_listed_while_more_are_asked_for`). Everything else found is
    held likewise: kept songs and albums read for the last settled text narrow via
    `still_answering` and `albums_still_answering` until the load for the new text lands; albums
    and artists MB named via `albums_still_answering` and `artists_still_answering` while it is
    asked: tab counts and strips do not fall to nothing and climb back per key
    (`albums_found_stay_listed_while_the_words_are_edited`).
  - *A failure says so.* A request refused or unreachable for the text in the box is not
    remembered; its text is `unreached_for` and `elsewhere` answers `Beyond::Unreached`,
    *MusicBrainz could not be reached* with *Try again* (`ask_elsewhere_again`) beside it
    (`a_search_musicbrainz_refused_says_so_and_is_asked_again_on_a_press`). `Unreached` carries
    the songs still listed (those the catalog kept), so a failure is said beside them (*N songs
    not in it so far; MusicBrainz could not be reached*), not hidden behind *Press a song to
    download it* (`a_search_musicbrainz_did_not_answer_says_so_beside_the_songs_kept`). A failure
    for a text typed past is a debug line only. Turning Online on asks for what the box holds;
    off drops the ask.
  The answer drawn is kept against the text it answered (`found_for`): a reload does not ask
  again unless only its songs half had come.
  - *What the catalog already knows comes first.* `Library::songs_kept_for` (songs of every
    library artist's unheld releases, learnt in the lookup pass, `library.md`) rides in the load
    with the text it was read for (`kept_for`): a song by an artist the listener has is listed the
    moment the listing is, before MB is asked, from what the catalog holds (only where Online is
    on; a press wants it from MB). `kept_before_the_rest` puts them first, drops an MB answer
    naming the same recording or the same folded title and credit
    (`songs_kept_from_a_discography_come_first_and_musicbrainz_does_not_repeat_them`).
    `restate_what_was_found` builds `shown` (what `found()` answers; the rows count) from both.
    Albums likewise: `Library::albums_kept_for` rides in the load (`kept_albums`),
    `albums_kept_before_the_rest` puts them ahead of `find_albums`' answer into `albums_shown`
    (what `albums_found()` answers) while the box holds the text either was read for
    (`albums_kept_from_a_discography_come_first_and_musicbrainz_does_not_repeat_them`).
  A search whose plain words a held track sings is offered as `lyrics:"…"`: `Library::sung` rides
  in the load; *Sung in N tracks* stands in the heading's actions and, where nothing else
  matched, in a pane's empty state, as a `search_instead`.
- **A search's reading is said under its words; the words as typed are a press away.** With
  `LibraryModel::meant` holding a `Meant` (`library.md`): the heading draws *Read as “You F O” by
  Stela Cole* in accent under the quoted words, *Reads* chips show the scoped search run, matched
  runs light by it, *Save this search* saves it, *Search the words as typed* is `search_as_typed`
  (literal until the box changes) (`a_title_by_an_artist_is_searched_as_that_title_by_that_artist`).
- **A pasted song or album link is downloaded, an artist link opens the artist; none is searched.**
  Search field is `catching(is_a_followed_link)`: a taken paste is emitted as `Caught`, not
  inserted; `ctrl-v` away from any field is `PasteAway`, a paste into the search box (one catch
  serves both). `RootView::follow_link` reads the `FollowedLink`, calls `Library::follow_link`
  (song) / `follow_album_link` / `follow_artist_link` on the background executor via
  `LibraryModel::follows_links` (the reference while Online is on, else a toast to turn it on);
  looking-up toast names which (`Told`). `followed`: song/album already held whole: toast; song
  found: `want_found`; album not held: `want_album` (as on an artist page); album held short:
  `want_missing_tracks` (album page's *Get the rest*); each lands in the sidebar downloads list as a
  press would. Box untouched, nothing searched. Unnamed link: toast saying song, album or artist
  nothing could name. Artist link opens the page: held: at once; else MusicBrainz-named via
  `land_artist_found` (as pressing a searched artist). Deezer, Spotify, Apple Music, TIDAL, YouTube,
  SoundCloud, Bandcamp, Amazon Music artist links are caught too, followed by the artist MusicBrainz
  files their page under. **A Deezer or ListenBrainz playlist link fills a playlist of what is
  held and wants the rest**: `followed_a_playlist` wants every found song (`want_found`, the
  downloads list), then `Library::fill_playlist_named` on the background executor puts the held
  songs into the playlist of that name (made where none is; a list of that name gains only what it
  lacks, so pasting the link again once the downloads land adds them), opens it and toasts what came
  of it (`playlist_told`); where nothing is held no playlist is made
  (`a_playlist_link_pasted_into_the_search_wants_every_song_it_holds_that_is_not_held`).
  (`a_song_link_pasted_into_the_search_is_downloaded_and_leaves_the_box_as_it_was`,
  `an_album_link_pasted_into_the_search_wants_every_song_of_the_album_it_names`,
  `pasting_at_the_window_puts_the_words_in_the_search`; links read: `library.md`; service asked:
  `online.md`.)
- **The search box narrows the Missing pane; its halves answer differently (one in the catalog, one
  not).** Both reads take the browse query, so the *Reads* row shows too and sidebar figure and list
  agree (SQL: `library.md`). Missing track: narrowed by the *album* it is short from, via the
  catalog grammar (type an album, its owner or a held track to see what it lacks). Unheld release:
  only its own row, narrowed by the fold of title, kind, first release date, or by artist (narrows a
  prolific discography by year and kind, no per-field control). Nothing matching: *Nothing missing
  matches* (not "nothing missing"), no lookup offered (the browse branch).
- **Playlists pane = an index plus one opened playlist, not a `Selection`** (`Selection` is
  Everything/album/artist). A playlist is `LibraryModel::opened`; its entries ride the same `Loaded`
  snapshot as albums, artists, tracks, roots (nothing else must know when one changes). Rows enter
  one via `RootView::hold_for_a_playlist` (picker: a queue row's + and the queue's *Save as a
  playlist*). Index/opened-playlist + enter track-browsing mode for the target; a track row's + adds
  directly; *Done*/Escape returns. The picker's listing also rides the snapshot:
  `Library::playlist_lists`, unnarrowed (often made from a search; a track found by name must reach
  any playlist) and list-playlists only (saved queries refuse rows; spares the per-query
  `count(*)`). So the picker reads nothing on its opening frame, a playlist started while open
  appears, and `adding` carries the `Held` alone. `Offered` is the arithmetic over it: the source
  playlist is stepped over by position, not filtered, so `uniform_list` draws `theme::PICKER_ROWS`
  (9) rows with no per-frame copy. `RootView::show_playlist` opens one with the
  `UniformListScrollHandle` `left_at` holds for its id (returning to a long one lands where read
  to). `set_query` installs a fresh handle when a keystroke narrows differently (old offset rows
  differ); `forget_what_has_gone` drops a handle once the listing stops naming its playlist, skipped
  while a search narrows the listing (what that leaves out is still there). Index cards/rows use the
  opened hero's mosaic; their + starts the target's track-browsing mode (no copy). The index's sort
  icon sits with icon-only Import and New playlist in the heading's right actions; it reveals/tucks
  the ORDER and READS choices under the heading. None of this, nor the listing's order, survives the
  run (tab and settings category do by default: `remember-tab`, `remember-settings-category`, both
  on).
- **One `Field` names every playlist.** `RootView::name` serves the pane's new-and-rename row and
  the picker's *or a new one*, so one is open at a time: `name_a_playlist` closes the picker,
  `hold_for_a_playlist` the naming row. It emits `Submitted` on enter; `RootView::name_given`
  decides from whichever is open: create, create-and-add, rename, or save/revise a search. The
  `Naming` variant picks the heading drawing the row: `Query` under *tracks* (where the gesture is;
  search box above stays live), `New`/`Rename` under playlists; one entity can't be in the frame
  twice, so each heading filters `naming` for the variants the other leaves. `RootView::contact` is
  the Online card's `Field`, filled by `Field::hold` (`set_content`, preedit cleared, `clear`'s
  shape): the saved contact is in the box when the window opens, put back trimmed on enter.
  `contact_given` stores `Setting::Contact`, moves the global, reports it is sent from the next
  request (the store reintroduces every client, `online.md`), returns focus. Press outside:
  `leave_contact` (gives typed text if it differs from stored); escape: `put_back_contact` (stored
  text back first). Same pair for AcoustID key, AudD and ListenBrainz tokens, Subsonic and TIDAL
  accounts, organise layout
  (`a_contact_typed_and_left_with_the_pointer_is_kept_and_one_left_with_escape_is_put_back`), ahead
  of the naming row in `dismiss_search`; `editing` counts them (typed keys stay off while a caret is
  in one). Order and cap ride beside the name as chips (a saved query = search, order, row cap; the
  window otherwise saves only the first). *Edit search* = `RootView::revise_search`: query text back
  in the search box, pane Tracks, chips filled from the saved query (revising is the saving
  gesture).
- **A run of rows is dragged where it goes; the keyboard reaches one before moving it.**
  `views/reorder.rs`: `Shift` names the list edited (`Queue`, `Playlist`, browse `Listing`); `Step`
  up/down, `Step::landing` the one arithmetic behind arrows, drop and keys (`resonate-ui`'s own
  tests cover it), stepping off `Span::first` going up, `Span::last` going down: span and row move
  by one rule. A row is drag source and drop target; dropping a span on row `to` = the arrows'
  `Command::Move` (take out, put back; never a swap). Payload `Carried`; `Ghost` under the pointer =
  first row's title plus how many more ride, padded by the offset gpui hands the constructor (rides
  at the pointer, not where the row began). `RootView::reach` is a `Reach`: `Shift`, the anchor, the
  row moved to (a queue reach is not drawn in a playlist; lives for the run only). `up`/`down` move
  and collapse it (queue opens on the playing row, playlist on its first); `shift-up`/`shift-down`
  grow from the anchor; `home`/`end` to either end; `pageup`/`pagedown` by `rows_a_page` (viewport
  height over `theme::row_height()`, grid: over `theme::grid_row()` times `grid_columns()`, read off
  `UniformListScrollHandle`'s `last_item_size`: `item` = viewport, `contents` = whole list);
  `ctrl-a` every row; `shift`-click every row between; `alt-up`/`alt-down` move what is reached,
  reach following; `enter` plays its first row; `delete` removes reached rows via the ✕'s
  `drop_rows` (gated on `Rows::are_edited` in a playlist, as the ✕). `backspace` on a reached row
  drops it too, not silently editing the search (as `typed` once did): `drop_reached` answers
  whether it acted; the query is edited only if not, unless a type-ahead jump put the reach there
  (backspace leaves it standing). `RootView::acting_on` is what every row gesture asks: the reach if
  the pressed row is inside it, else that row alone (✕, arrows, queue marks need no selection of
  their own). Drawn as a 2-unit accent edge plus faint accent wash; every reorderable row reserves
  the edge in `theme::UNMARKED` (no layout cost when it moves). Saved queries draw none (order is
  the query's). A drop is the one gesture not asking `acting_on`: `reorder::movable` takes the row
  it stands for beside the `Carried` it would hand over, so a span lands on the row the pointer let
  go over, not what that row would have carried if dragged.
- **A track or album is dragged out of a listing into the queue or a playlist.** A track row
  (`track_row`, every listing drawing one) and an album cell carry a `reorder::Lifted` (`Lift::Tracks`
  or `Lift::Album`, ghost titled as a row's) through `reorder::liftable`; a row of the tracks pane
  standing in the reach carries every loaded row of the reach (`acting_on`), as its ✕ would.
  `reorder::takes_a_lift` makes a target of a queue row (inserted at its place, `Placement::At`), an
  opened playlist's row (`LiftedTo::PlaylistAt`: `Library::insert_into_playlist`, appended then
  reseated in one undoable edit whose undo window runs from the place), the queue pane's body and
  an empty queue (`Placement::Queued`), the playback bar's queue button (likewise: the one target
  standing over every pane), and a list playlist's index row, card and pinned sidebar row (rows
  added at its end; a saved search takes none). `RootView::lifted_into` lands tracks at once and
  reads an album's rows in album order on the background executor first. The innermost target
  takes the drop (gpui takes the drag from the first listener), so a queue row places and the body
  under it appends
  (`a_track_dragged_onto_the_queue_button_is_queued_and_an_album_onto_a_pinned_playlist_fills_it`,
  `a_track_dragged_out_of_the_reach_it_stands_in_carries_the_whole_reach`,
  `rows_put_into_a_playlist_at_a_place_land_there_and_one_undo_takes_them_out`).
- **A drag scrolls the list; a held pointer keeps it scrolling.** `reorder::follows_a_drag` puts an
  `on_drag_move` on the div wrapping each `uniform_list`, handing `RootView::creep` a `Creeping`
  (scroll handle, list bounds, row count) while the pointer is within `DRAGGING_EDGE` (36) of an
  edge, else `None`. gpui reports drags only on movement, so `creep` scrolls one row past
  `ScrollHandle::top_item`/`bottom_item` then runs `creeping_on`: a task repeating every
  `DRAGGING_STEP` (60 ms) until `creeping` clears or `App::has_active_drag` says the drag is over.
  One task serves both ends and panes, reads `creeping` each turn (not captured), and alone clears
  the cell: live cell and live task mean each other.
- **Settings pane: a rail of categories over a column of sections; one table says what a section
  is.** `views/settings/` is a folder. `find.rs` = vocabulary: `Category` (Output, Processing,
  Equaliser, Library, Filters, Online, Desktop, Appearance, About) and `Group`, one per setting,
  carrying category, title, hint, *also*-called words, the `SettingKey`s putting it back. Nothing
  else enumerates sections: `Category::groups` filters the one table, `mod.rs::group` dispatches a
  `Group` to its drawing method; tests hold every group to one category, a unique title, its own
  hint, no key claimed twice. Rail = `theme::settings_rail()` icon rows down the left edge;
  `RootView` remembers the open one for the run; body = a column of `kit::section`s (header strip:
  title, revert mark, info mark; over a body), not flat cards. Body scrolls under
  `RootView::settings_scroll` (own `ScrollHandle`); `show_settings` resets it to top on category
  change (gpui keys scroll offset by element id; one `"settings"` id left the next page as far down
  as the last was read).
- **Filters = the library's extension and minimum-length selection** (groups `MusicExtensions`,
  `MinimumLength`). Extensions: one wrapping multi-select of `kit::chip`s, each with a checkmark
  when included, all selected by default; the checkmark slot keeps its size when cleared (no extra
  room). A count and *Select all*/*Clear selection* actions (greyed where they would change
  nothing); chips and actions take the ordinary focus ring. Minimum: seconds field, 0 to 600,
  applied on Enter or leaving (positive minimum excludes tracks of unknown length); a refused number
  stays in the field and toasts; Escape restores the held value and returns focus. Changes reload
  listings at once and store via `SettingsWriter`; group and category can be reset through the
  ordinary controls.
- **Files dragged in from outside are weighed while over the window and copied when dropped.**
  `views/dropping.rs`. gpui makes a file drag an `ExternalPaths` drag, but only the drop target
  hears it end (nothing says the drag *left*), so the root's `on_drag_move::<ExternalPaths>` sets
  `RootView::incoming` (paths, and `resonate_library::weigh`'s read of each once it lands) and
  starts `watching_the_drag`, a 100 ms timer clearing it once `App::has_active_drag` is false.
  **Nothing on a drag or a frame touches the disc on the render thread**: `weigh_the_drag` reads the
  paths on the background executor (the card says *Looking at what is dragged…*,
  `Verdict::Weighing`, until it lands); whether the music folder is there is
  `RootView::music_folder_is_there`, a `FolderStanding` looked at on the background executor at most
  every `FOLDER_LOOKED_AT_EVERY` (2 s) while something draws it (overlay, Library settings), the
  last answer standing meanwhile; a drop reuses the drag's weighing and looks at the folder once
  more off the thread before `land_the_drop` acts. A folder on a stalled mount stalls a background
  thread, never the window. While
  `incoming` is set, `drop_overlay` draws over the window and is the occluding `on_drop` target: a
  scrim, a dashed card listing up to six names each with *copied*, *cue sheet, copied*, *beside a
  song, copied with it*, a folder line, *not audio, left out* or *not there*, and the destination
  (*copied and filed by your layout* where `file-dropped` is on). `verdict` = the one decision
  overlay and drop share: `Ready`, `NoFolder`, `FolderGone`, `NothingToTake`, `Busy`; the last four
  draw in the failure colour and toast when dropped; no `music-folder` also opens Settings on
  Library. Ready: `take_in` into `ResonateApp::music_folder`; `copying_pill` draws progress (files,
  bytes, *Stop*); `told_of` toasts the outcome; where anything was copied, the root reaching the
  folder (or the folder, added as one) is scanned via `add_roots`, with `file-dropped` on the landed
  songs first handed to `file_once_scanned` (filed once that scan lands; `library.md`'s *Taking
  files in*). The handle lives in `ResonateApp::copying`, a `Copying` slot the progress task takes
  it from once finished; at event-loop end `run` winds it down (`wound_down`: cancel, wait up to
  `WIND_DOWN_WITHIN`, 10 s) before the library's own pass. A file being copied is
  `.<name>.<pid>.resonate-part` until renamed into place whole, so a copy outlasting the wait leaves
  at worst that staged part. gpui can't build an `ExternalPaths` in a test: `driven.rs` calls
  `dragged_over` and `dropped` on the `RootView` instead.
- **Discord = two Desktop groups; they write a global before a file.** *Discord*: switch and
  application id. *What Discord shows*: how much text, which picture, the icon, the bar, whether a
  pause keeps it. Each control changes `ResonateApp::presence`, stores its `Setting`, hands the
  whole `Presence` to `Present::follow` (publisher starts/stops/re-sends at once). Id and icon are
  `Field`s read on enter via `AppId::parse`/`Icon::parse`; unreadable text is a `Notice::Trouble`
  storing nothing (never a key the next start refuses); blank clears the key.
- **The body is as wide as a number, never `w_full` under a `max_w`.** taffy fixes a flex item's
  height from a measure taken before a percentage width resolved: such a column is measured with its
  text *unwrapped*, its `content_size` comes out short, a long category's bottom can't be scrolled
  to (the lyric row's trap, same answer): use a pixel width wherever a column's height depends on
  text wrapping.
- **A boolean is a `kit::switch`, a choice a `kit::segmented`; only a device or palette is still a
  row.** The four near-identical two-variant enums are gone: a switch names what it does and what
  off means, whole row = hit area; a `Choice` is a trough of segments. Vocabulary: `kit::section`,
  `field`, `segmented`, `switch`, `preview`, `dot_swatch`, `choice_row` with its `radio`; the pane
  draws none itself (`kit::button`'s rule). **`kit::segmented` answers with the trough itself; the
  caller hugs it.** It once answered with a wrapper round the trough, so every segment landed beside
  it (bare labels, an empty six-pixel pill to their left, in every selector). Watch for builders
  returning a parent their children will not reach.
- **A device reads as what it is and what it takes; its node name is behind the pointer.** Each
  choice, *Follow the system default* included, is a `kit::choice_row`: `kit::radio` held
  `kit::level_with_the_choice` beside a column whose first line is a `kit::choice_name` (radio on
  the name, not midway down three lines; default and devices share one edge). Chosen row: whole
  accent border over an accent wash, like the worn palette card; that and the filled radio are its
  only marks (the identical per-row speaker icon and the 2-unit accent edge were a third and
  fourth). Name is `flex_1`, ends in an ellipsis (sized by content: a clamped name collapsed beside
  its badges; `truncate` alone sliced its last letter); badges follow on the same line (`DEFAULT`,
  `UNPLUGGED`, `IN USE`, the playing track's verdict `TAKES … AS IS`/`CONVERTS`), so no description
  pushes them off and verdicts read as one column. Under the name, two `kit::details` lines, parts
  divided by faint `·`: how the sink is reached, in words (port, the card's profile, whose volume it
  is) as faint `kit::detail`s, each ending in an ellipsis where it outruns the line (the default
  entry's *Whatever PipeWire routes to, which is …* was sliced by the card's edge); and what it
  takes, in figures (depths/rates via `format::depth` and `SampleRate::kilohertz`: core's, every
  digit the rate holds, so 22.05 kHz is not 22.0, and visualiser, analysis and Settings read as
  core's `Display`; comma-listed) as `kit::figure`s. Replaced: five identical grey mono badges in
  one wrapping cloud (words in the figures' face, slash-listed, reading as shorthand). A port with
  nothing plugged in = a badge and a `muted` name, not a parenthesis; the default entry says which
  device PipeWire routes to now where the graph names one. The node name is what the settings file
  holds, so the row `names` itself by it; `SinkInfo::is_hardware` is drawn nowhere (reads
  `device.api` off the node, which an ordinary ALSA sink lacks).
- **A section holds a subject, not a control.** Theme shelf and accent swatches = one *Colour*
  section with a `kit::field` label over each half (six one-control sections read as a list of
  switches, not a page). A group's hint is written once, in the table; the header's info mark is its
  only drawing (`views/hint.rs`: a gpui tooltip is built from an `AnyView`, not a string).
- **Every group ends in a line saying what it does; a choice says what the chosen option does.**
  `Choice::meaning` is required (no default); `RootView::choices` draws the chosen option's meaning
  under its trough as a `note` (resampler filter, dither, ReplayGain mode, buffer depth, Discord
  picture); `detail` stays the hover for figures such as the filter's taps. A list/button-row group
  (devices, music folders, scan, bindings, About's three readbacks) ends in a `note` of its own;
  every note is the one `note` builder at `text_sm`, accent line included. *Lossy sources* =
  *Artificially enhance lossy files*, with the `EXPERIMENTAL` badge `Group::is_experimental` puts in
  a header (what it adds is a guess at what the encoder threw away); Off, Repair, Repair and extend
  each say in words what they take or add; `LOSSY_ONLY` says which files are touched and that a
  repaired one is no longer bit-perfect.
- **A palette is shown, not named.** `kit::preview` = a strip of four bands (theme's sidebar, panes,
  what it raises above them, the accent that would be worn) under the palette's name in a card with
  accent border when worn. An accent is a round `kit::dot_swatch` with a check in whichever ink
  `theme::ink_over` says reads on it. Appearance is the one category with nothing behind it but the
  window: a choice is worn via `theme::wear`, saved through the `Settings` seam, redrawn by
  `cx.refresh_windows()`; no `Command`, and the pane marks what is worn, not what was last sent.
  Text size likewise moves every measure. Groups in order: *Colour*; *Layout* (text size); *Window
  buttons* (chrome = how the window is drawn; Desktop = what the player tells the session), worn via
  `RootView::show_window_buttons` onto the global, not `theme` (a button is no colour or measure);
  *The volume wheel*: one switch onto `ResonateApp::scroll_volume`, `scroll-volume`; *Mouse
  navigation*: one switch onto `ResonateApp::mouse_navigation` (`navigate_with_mouse_buttons`),
  `mouse-navigation`; *Scrollbars*: Always / While scrolling (`ScrollbarMode::AutoHidden`, default)
  / Never onto `ResonateApp::scrollbars`, `scrollbars`; *Sidebar tabs*: three switches onto
  `ResonateApp::tabs` via `RootView::show_tabs`, `suggestions-tab`, `missing-tab`, `tab-counts` (the
  last takes the figure off every sidebar tab); *Window state*: `remember-tab`,
  `remember-window-size`, `remember-settings-category`. `Pane::is_shown` is what a tab being off
  means: sidebar and `stepped_pane` skip the pane, `set_pane` lands on Tracks instead (no way back
  or button reaches a hidden pane); hiding the front pane moves off it at once.
- **gpui scrolls a region and draws no bar, so `views/scrollbar.rs` does.** `Scrollbars::of` reads
  the setting once where a pane is built; `vertical`/`horizontal`/`around` answer a bar over the
  region's edge, or an empty absolute div under `Hidden` (a pane is built one way either way). A
  `horizontal` bar is inset `SHELF_INSET` (24) each side, the padding every shelf strip (search's,
  artist page's) lays cards in by, so the thumb runs under the cards, not to the strip's edges. A
  bar reads the region's own `ScrollHandle` (a `uniform_list`'s base handle) and paints the thumb in
  a `canvas` (offset moves between renders; only paint sees where it is now). **Nothing a bar knows
  survives a render:** a builder-made `Cell` is new each frame, so an `on_hover` writing into one
  was gone by the redraw it asked for. The bar lights by weighing `Window::mouse_position` against
  its bounds in paint, and paint writes those bounds into a cell the same frame's listeners read
  (track press and drag grip measure the bar the pointer is over). Where on the thumb it was held is
  taken when the drag *starts* (in `on_drag`'s constructor), not on press (a track press moves the
  thumb and redraws before the drag begins). **The lyrics pane's bar draws only while the sheet is
  moved:** a following sheet glides its whole length and a thumb beside the words read as a second,
  busier line; `Scrollbars::vertical_while` paints the thumb only if the pane says it moved
  (`LyricsModel::moved_by_hand_lately`, `BAR_LINGERS` 1.5 s after a wheel turn) or the pointer is on
  the bar/thumb held; the glide is no movement, and `is_turning` keeps frames coming until the
  linger is out. **`ScrollbarMode::AutoHidden` does likewise for every bar, from the offset alone:**
  an always-drawn bar is drawn `Shown::WhileScrolled`; its paint keeps the last offset in element
  state under `LAST_SCROLLED` (the one thing a bar carries between frames, as a `Cell` doesn't
  survive; read every paint, pointer-lit or not, since gpui drops state no frame reads) and paints
  the thumb while that offset moved within `SCROLLED_LINGERS` (1.2 s) or the pointer is on the
  bar/thumb held. A paint seeing the offset move spawns one timer refreshing the window when the
  linger is out (no frames asked between). The lyrics bar keeps its own rule under every mode but
  `Hidden`.
- **A setting off its default says so; the mark puts it back.** `defaults.rs`: `Standing` =
  everything the pane can put back (`OutputSettings`, `Appearance`; online, lyrics, listening,
  resumption, desktop, Discord, convolution, window, filter, tab, mouse and scrollbar switches and
  choices; whether each typed value is given: contact, keys, tokens, template, inbox, music folder,
  Subsonic, TIDAL); `differs` weighs it against `Standing::as_built` (`EngineConfig::default()`,
  `Appearance::DEFAULT`, …); `puts_back` answers the restoring commands. A differing section grows
  an `Icon::Undo` in its header; the footer has *Reset <category>* for every category with a key to
  put back; About's *Reset everything* arms on first press, fires on second (no modal in this
  crate's vocabulary). Putting back = send the default and take the key *out of the file* via
  `SettingChange::Forget` (a build whose default later changes is followed, not pinned). **Settings
  are written off the render thread; a run is one write.** `RootView::store`/`forget` hand a
  `SettingChange` to `SettingsWriter`, which sends what gathered to a thread of its own
  (`resonate-settings`) as one batch, one in flight, arrivals meanwhile the next; `Settings::apply`
  takes the whole batch. So *Reset everything* (~50 changes) is a write or two, not a locked
  read-modify-write with two `sync_all`s each on the UI thread, and dragging a slider costs the
  frame nothing. A failed batch is one toast; dropping the writer (window close) sends what is
  gathered and joins the thread (nothing changed last-moment is lost). A keyless group (folders,
  scan, *Look up now*, all three of About's) offers no mark (test-held).
- **A setting is found by typing, not by remembering the tab.** `RootView::finding` = the pane's own
  `Field`, reached by `ctrl-,` or a press, counted by `RootView::editing` (transport keys stand down
  while its caret is in it), cleared and blurred by escape ahead of the contact field in
  `dismiss_search`. `Narrowing` matches every word against a group's title, hint, category, other
  names (`sink` finds the device, `latency` the buffer, `sacd` DoP). With a query, the heading reads
  *Found*, the rail draws each category's match count instead of a selection, and the body draws
  matching sections from *every* category under a category eyebrow (a filter inside the front tab
  alone is a worse tab). **Ranked by how they answered:** a group answers each word by title, other
  name, category or hint, in that order of strength, weighed as its weakest word; a category leads
  by its best group, a group within it by its own weight, so *dither* opens on Dither, not a shaping
  hint mentioning it, and no category is drawn under two eyebrows
  (`a_setting_named_by_the_words_leads_one_whose_hint_merely_mentions_them`).
- **A right press answers with a menu; `views/menu.rs` is all of how.** `Menu` = position + entries
  (icon, label, optional key, closure); `Menu::at(…).does(…).under(…).apart()` builds,
  `menu::opens_a_menu` attaches to any element: a pane says what a press offers, never how a menu
  draws. Renders as `deferred(anchored().position(at).snap_to_window_with_margin(…))` over an
  occluding scrim taking the outside press; one `Option<Menu>` on `RootView`, one arm in escape's
  chain (after the sheets, before a toast), one at the head of `dismiss_search` (`adding`'s and
  `magnified`'s shape). Not built on `tooltip` (forbidden by `hint.rs`'s source-walking test;
  neither press nor scroll takes a tooltip down). No key context: `up`, `down`, `enter` already name
  actions this window handles, so `reach_row` and `play_reached_rows` step/press the menu before
  reaching for a row. **`on_click` never fires for a right press** (gpui 0.2.2 records a pending
  press only for `MouseButton::Left`): a menu-opening press plays, scopes, seeks nothing, and no
  listener sharing an element with a menu needs a guard (an `is_right_click` guard once stood on
  every such listener and could never answer false; removed). A gpui routing other buttons to
  `on_aux_click` keeps that promise. Track row: plays, queues either way, holds for a playlist,
  reaches artist and album (*Go to album* lives only here: a track row has no album column), shows
  its file in the file manager, copies path, title, artist, album. **A row's destination is read
  when the entry is pressed, not when the menu opened:** `Menu::reaches` takes the row's catalog id
  beside the album and artist it drew; the press asks `LibraryModel::standing_of` for the track as
  the catalog holds it now (a menu open across a scan that gathered the album away opens the album
  it was gathered into); a row no scan has seen, or since dropped, goes where the menu said.
  `Menu::offers_the_file` is that last group, written once for tracks, queue and playlist rows,
  given the row's `Called`, whose album `RootView::album_named` reads off the catalog's album or,
  for an unscanned row, the tags the player read (empty name: not offered). *Show in the file
  manager* = gpui's `reveal_path` (portal `OpenURI.OpenDirectory`, folder with the file selected;
  falls back to opening the folder). **The playback bar's cover, title, artist and album open one
  menu**, `what_plays_menu`: cover's whole view (cover only), inspector, artist and album,
  favouring, sharing, the file group with copies (copying a name whole, not the bar's cut, is one
  entry away). Inspector title: left click to its album, right click copies it whole via
  `copied_on_a_right_click` (toast says what is on the clipboard). Album cell, artist row, the two
  covers, queue row, playlist row, column header, search box, lyric line each carry their own menu;
  a sidebar row, settings control and missing row deliberately carry none (each would offer only
  what one press does).
- **The reached row's menu opens from the keyboard** (`menu`, or `shift-f10`: `OpenTheMenu`). A row
  drawing a menu through `menu::opens_a_reachable_menu` with an `AtTheReach` (the reach's moving
  row, `RootView::at_the_reach`; queue, playlist and track rows, album and artist cells and rows,
  playlist cards and rows) lays a canvas whose prepaint keeps its bounds and its menu's builder as
  the `ReachedMenu` global; `open_the_reached_menu` builds that menu only where the global names the
  reach in force, so a row scrolled away or a reach since moved opens nothing stale. It opens in from
  the row's left edge at its middle, its first entry reached, so the arrows and `enter` press any
  entry (*Add to playlist…*, *Go to artist*, *Share*, favouring) as the right press would
  (`the_menu_key_opens_the_reached_rows_menu_and_the_arrows_and_enter_press_an_entry`).
- **Copies go to the compositor over `ext-data-control`, not gpui.** gpui 0.2.2's Wayland
  `write_to_clipboard` sets the selection under the last *key* press's serial; a pointer copy (path,
  lyric line, share) carried none or a stale one, so KWin kept the old clipboard. `clipboard::copy`
  is the one way to copy: own connection, binds `ext_data_control_manager_v1` and the seat, sets a
  source offering the text types, waits `ANSWERED_WITHIN` (250 ms) for the round trip saying the
  compositor took it; thread `resonate-clipboard` then answers each `send` until the next copy
  (anyone's) cancels the source, so at most one lives. No data control (GNOME restricts it): gpui's
  own write, which works whenever the window was last reached by keyboard. **The UI thread never
  waits; a late answer cannot overwrite the fallback.** `copy_through` starts the thread plus a
  foreground task racing its oneshot answer against an `ANSWERED_WITHIN` timer; the fallback is
  written only once the compositor refused or the timer won. One `Claim` (atomic, settled once)
  decides: thread claims just before `set_selection`, task gives up when the timer fires, loser
  stands down (thread finding it given up destroys its source unset; task finding it claimed waits
  for the round trip). Else a `recv_timeout` blocked the frame 250 ms and a late thread set the
  selection over the fallback's text. **A later copy wins however round trips land.** Each copy
  takes a ticket from `Copies` (per `App`, not static, so tests' windows do not outrun each other);
  only the newest may claim or write the fallback. Claim, `set_selection` and round trip run under
  `Issued::handing_over`, one lock across copies, so an older slow-connecting copy stands down
  (`a_copy_outrun_by_a_later_one_neither_claims_the_compositor_nor_writes_the_fallback`);
  `clipboard.rs` tests drive both sides with a stand-in offer. `wayland-client` +
  `wayland-protocols` (`staging`), already linked by gpui; no `unsafe` (descriptor arrives as
  `OwnedFd`, written through a `File`). Proved against headless `kwin_wayland --virtual` on its own
  socket via `wl-paste`; not a test (a desktop copy is the listener's clipboard).
- **A scope remembers where it was opened from; its way back names that place.** `Wayback` = pane,
  selection, row the list was left on, place's name, in a `WAYS_BACK` (16) stack; every scoping
  gesture goes through `RootView::opened`. `RootView::here` reads the name as the scope is left
  (album title, artist name, pane label): by draw time the library has moved on. `goes_forward` is
  the bounded forward stack (going back saves the current page there, going forward saves it to
  `came_from`); a new scope, another category or `show_everything` clears it. With Appearance's
  *Mouse navigation* on (`mouse-navigation`, default true) the root maps the mouse's back/forward
  buttons onto the same stacks. **A category is a place too**: `choose_pane` (sidebar press, tab
  stepped by key) keeps the page it left in `came_from` whenever pane or selection moved, and holds
  the stack through the `show_everything` an unscoping landing runs, so mouse-back returns Albums to
  the Tracks it came from as well as album to list
  (`the_way_back_and_forward_crosses_from_one_category_to_another`). Escape (`RootView::step_back`)
  on an unscoped page steps back only where the way back leads to a scope (`goes_back_to_a_scope`),
  never flipping tabs. The way back is `kit::way_back` at the page's top left, chevron + name (*‹
  Tracks*, *‹ Hypnotize*); with nothing left behind it, it reads the category the page stands under
  and goes there, clearing the scope (replacing a ghost *Show all* that cleared the scope rather
  than returning). **A list is left at a row and how far into it.** `LeftAt` reads the scroll
  offset against `row_height_of` (grid row: albums, artist grid; tall row: artists list; plain row:
  tracks) as row + fraction past; landing sets the offset back from the same pair at the rows'
  height then, so a list lands on its pixel, and one whose rows grew with the text size on the same
  row as far into it (`the_way_back_lands_on_the_pixel_the_list_was_left_at`, driven;
  `a_list_whose_rows_grew_lands_on_the_row_it_was_left_at_and_as_far_into_it`). Landing waits on
  purpose (listing read on the background executor): `land_where_it_was_left` holds until the pane
  has that many rows. Albums, tracks, artists all land, each via its own scroll handle (`album_rows`
  the grid's), so the row is read off the list that was showing, not `track_rows`. `landing_on` is
  set only for a `Pane::lands_where_it_was_left`; `set_pane` clears it (a landing no pane reads must
  not wait and jump the artists pane to a stale row later).
- **A queued row's name is weighed against the row, not only its id.** The engine mints ids from
  `u64::MAX` down afresh per restore, unscanned playlist row and doubled row: two files can carry
  one id a load apart. `LibraryModel::track_of` keeps location and span beside the name it read and
  re-reads where they disagree. `album_title` answers for any album (scoped, listed, or read by id
  into a bounded `read_albums` the next listing load discards), so a search or page leaving the
  playing album out does not blank the playback bar, magnifier caption or queue's album order.
  **The queue pane's per-row weighing is off the render thread.** `queue_heading`'s total is
  measured once per queue and library revision in the background (`queued_rows`, `QueueMeasure`);
  the heading keeps the last total until the new lands, reading *at least* it where a row has no
  scanned length (`QueueLength::unmeasured`). A sort chip keys every row in the background too
  (`ordered_rows`: tracks one pass, album titles another via `Library::album_titles`), sending the
  order only if the queue is still the revision it keyed. Both once ran on the UI thread one row at
  a time past `NAMES_HELD`'s 4 096: a counted play under a 20 000-row queue froze the window for
  ~40 000 reads.
- **A row is keyed by what it holds, not where it stands.** gpui keeps tooltip, hover and drag in
  element state under the id path: an index-keyed row handed row N's tooltip (a favourite's *Take
  out of favourites* among them) to whatever an edit moved into N. Keys: queue row = queue id; track
  row = `TrackId`; artist row = `ArtistId`; missing row's want mark = release track or found
  recording; playlist entry (may hold one cut twice) = `listing::keyed_by` over cut and place. A
  row now holding something else is a new element, nothing carried over.
- **A row gesture reads the row the pane drew, not the list's row at that index.** A search-narrowed
  playlist is not reached, so its row menu's *Take out of the playlist* drops
  `Span::one(entry.position)` (the row's place in the list), as its ✕ does; `index` is its place in
  the narrowed view. Enter on a reached row inside an album goes through `LibraryModel::played_from`
  (display row to the track it draws; nothing for a disc heading or missing row), as a click reads
  `ListedRow::Held` through `played_from_held`. Tracks-pane rows draw `Plays::AsTheListingIsDrawn`;
  favourites' and a suggestion's `Plays::TheseRows`.
- **Every control is keyboard-reachable through gpui's own ring.** `views/focus.rs` holds one
  `FocusHandle` per control id, forgetting those a frame stopped drawing (a redrawn control keeps
  the caret); ring = `Window::focus_next`/`focus_prev` over tab stops gpui inserts in paint order
  (no re-implemented order). A control carries `.track_focus`, `.tab_stop(true)` and
  `key_context(CONTROL_CONTEXT)`, the whole key plumbing (`RootView::ringed`): `space`/`enter` bound under it fire the
  control's *own* `on_action`; escape lets go; every window binding that carried `!Search` carries
  `!Search && !Control`, so the transport stands down as for the search field. `tab`/`shift-tab`
  are bound with no predicate (focus must move from inside a text field too). Focus is drawn as
  hover is: accent on the border where a control has one, else an accent wash.
  - **A tab stop is the handle's, not the element's.** gpui reads `FocusHandle::tab_stop` for a
    tracked handle and applies an element's `.tab_stop(true)` only to a handle it made itself, so
    `Controls::at` and `Field::new` make their handles tab stops (until they did, no control or
    field was in the ring at all and `tab` moved nothing).
  - A pane's controls are `controls`: `RootView::content` opens each pane frame with
    `Controls::opening`, which keeps the handles the frame before drew and starts counting again,
    so a control a `uniform_list` row draws (laid out after `content` returns) keeps its caret
    across frames. Settings lays its own through `in_the_ring`; every other pane through
    `RootView::in_the_pane_ring`, which names the handle by the control's own element id, presses it
    through the one closure a click and `space`/`enter` share, shows it while it holds the caret
    where it hides until hovered (`opacity` 1 on focus) and takes no caret on a click: heading
    buttons and marks, segments and chips, the way back, a row's star, queue marks, ✕ and pin, the
    playlist marks. A row's hidden controls are `RootView::row_controls`, a container tracking a
    handle that is no tab stop (`Controls::holding`), shown by `in_focus` while one of its controls
    holds the caret
    (`tab_reaches_a_headings_play_and_a_rows_star_and_enter_presses_them`).
  - **The seek and volume rails are tab stops of their own context.** `slider::rail` tracks a
    standing handle under `Control Rail`; `app::answering_on_a_rail` binds `right`/`up` to `RailOn`
    and `left`/`down` to `RailBack`, which step the rail it is on: a seek of `seek_step`, a volume
    of `VOLUME_STEP` (`a_rail_holding_the_caret_moves_with_the_arrows`). A press on a rail takes no
    caret.
  - `standing_controls` hold what every frame draws (transport buttons, the mute mark, sidebar
    panes), never pruned (a pane's frame would otherwise drop the transport's handles and the
    caret with them); `in_the_standing_ring` lays them. A click on one takes no caret
    (`prevent_default` on its press: gpui focuses a focusable element under a press), so choosing
    a pane with the pointer leaves the arrows reaching its rows, not stuck on the sidebar.
  - `tab` from the window (the results the search box handed the caret to) moves past the search
    field, `shift-tab` returns to it: `tab` in the field means *go to the results*, so a ring
    starting over at the field from the window never got further
    (`tab_reaches_the_transport_buttons_and_the_sidebar_and_enter_presses_them`).
- **What is in this build, and where it keeps things, is a category, not a window.** `WindowKind`
  is `Main` alone (no `Preferences`/`About`: nothing constructed them, a defect per `errors.md`).
  About reports the version, the two faces `fonts.rs` settled on, the sink count the graph
  advertises, whether a reference started; names the settings file and the catalog, reached as
  `Places` on `run` beside the `Settings` seam in `Stored`. Readbacks, not settings: `library` has
  no control (it names the database the pane reads); the settings path is what `--config` chose.
- **DoP is a hardware claim; the pane says so in as many words.** `Command::SetDop`,
  `OutputSettings::dop`, `Setting::Dop` exist because the key did: `EngineConfig::dop` and
  `ConfigKey::Dop` were readable from `config.toml` and reachable from nowhere else. Off by default;
  the hint repeats `audio.md`: nothing in ALSA or SPA advertises DoP, only the DAC's own detector
  knows, and a DAC not decoding it plays the markers as full-scale white noise.
  `pipeline.rs`'s `no_setting_the_pane_offers_can_produce_a_marked_plan_with_a_stage_in_it` already
  fixed `dop: true`, so its claim covers the pane.
- **A value off a choice's table is said, never left unlit.** `RootView::choices` takes the value in
  force, not an `Option`: a segment lights where it equals one of `Choice::ALL`; where none does,
  `said_under` writes *… is in force, which is none of these.* in place of a meaning, via
  `Choice::in_force` (label by default, which a closed enum always has; the figure itself where the
  key reads any number: `buffer-ms`, `listen-for`, `bluetooth-lead-ms`, `bluetooth-awake-s`, the
  two ReplayGain trims, a `history-kept` of days the table lacks). Those choices are newtypes over
  what the key holds (`BufferDepth(Duration)`, `ClipLength(Duration)`, `PreAmp(Trim)`), not enums
  of rungs, so the pane cannot turn an off-table value into `None` and forget to say so; each
  module's tests hold the sentence. The resampler group prints `SincParams`' four numbers on hover,
  no adjectives. A device named in the file but absent from the graph marks no row; the group says
  so. Volume is debounced (`VOLUME_SETTLE`, 400 ms): a held key rewrites `config.toml` once; it is
  the playback bar's control, not a setting here.
- **Whether a control can be pressed is the builder's decision.** `kit::Press` and
  `settings::action`: a greyed control is built greyed, takes no listener, joins no focus ring
  (gpui's `hover` has a `debug_assert!` a second call would trip; a control nothing can press is no
  tab stop).
- **A pass that rewrites files is armed before it runs; the preview arms it.** *Organising* and
  *Tagging* share one shape: *Preview* runs the pass with `Pass::Preview`; *Apply* is greyed until a
  preview landed, then takes two presses (the second under a note saying what the first armed);
  *Stop* is drawn only while it runs. Each also offers *Put the last run back* wherever the library
  notes a run (`walks_back`, `tags_walk_back`), armed the same way under `walking_the_filing_back`
  and `walking_the_tags_back`. `RootView::moving_the_files` and `writing_the_tags` are the other
  arming flags. `disarm` (leaving the pane, changing category) runs `lower_every_armed_press` (these
  four, the reset, the vault's keep, the curve discard, history aging) and clears a finished reset's
  note. The pointer leaving the settings body does the same lowering from `on_hover` the moment it
  reads false (a transition: a press armed by keyboard with the pointer already elsewhere stays
  armed until the pointer has been in and out), leaving a finished reset's note (no arming). Both
  passes run through `Work`, read by `LibraryModel::is_busy`: scan, forget, organise, tag write
  cannot overlap, each greying the others' controls. `Library::retag` takes the same `Walk` guard
  as `Library::scan` and `Library::organise`, so a second caller would be refused anyway; the
  greying stops it being asked.
- **A preview is taken down once the catalog it read has moved.** `Planned` (held by *Organising*,
  *Tagging*, the vault's *Import*): `Not`, `Shown` with the `resonate_library::CatalogStamp` taken
  as the preview started, or `Outdated`. Every reload's `landed` weighs a shown preview against
  `Library::plans_stamp`; stamp moved = drop rows and counts, grey *Apply*, group says the catalog
  moved (not "ask for a first preview"). The stamp is a counter of its own on the writer connection
  (temp triggers over the columns a plan reads, `PLANNED_FROM` in `db.rs`) beside `PRAGMA
  data_version` for another process: a lookup landing a release or a scan reading a file anew takes
  the preview down; a counted play or favourite does not. An arming = flag *and* a shown preview, so
  a taken-down preview leaves no half-armed *Apply*.
- **The Tagging group draws the plan, not a count of it.** `listed` = first `WRITES_SHOWN` (12) of
  `Retagging::writes`: one raised row per file, file name over the fields to be written (`cover
  art` among them where a picture goes), *and N more* where the pass named more, then the summary
  note. A count is the wrong preview for a pass whose unit is a field (arming *Apply* wants which
  files and what goes in them). The count stays the pass's own (`would write`, fields and pictures
  summed off the plan before an apply; `RetagStats` after): note and rows cannot disagree.
- **What a scan skips can be re-read on purpose, one part at a time.** Library category's
  *Refreshing* group: *Read every file again* (ordinary scan with `incremental` off,
  `Reading::Everything` beside `Prompted` on `LibraryModel::start_scan`): every file re-probed
  whatever size and mtime say; names, lengths, sheet cuts, embedded covers, search index follow,
  each row keeping id, plays, favourite. *Look for missing covers* (`Library::ask_again_for_covers`
  + a lookup): sweeps the archive for every album with a release and no picture. Both grey with the
  scan's `is_busy`, the covers button with `can_enrich` too. What MusicBrainz answered is Online's
  *Refresh all*, which the group's note points at.
- **Library roots are edited through the desktop's file picker, or typed.** `App::prompt_for_paths`
  with `directories: true` is the XDG portal; a machine without one says so and points at the field
  beneath, `RootView::typed_root` (a `Field` like the organising template's, *Or type a folder,
  then press enter*). `root_typed` reads a leading `~` as `$HOME`, refuses a non-directory in the
  pane's own notice, hands a directory to the picker's `add_roots`: a portal-less session adds
  folders from the window, not only `resonate scan`. Adding scans just the folders named
  (registering them); *Rescan* walks registered roots; dropping one goes through
  `Library::remove_root`, forgetting every track from it. Roots ride in the one `Loaded` snapshot
  with albums, artists, tracks, so nothing else needs to know when they change. They live in the
  library database, not `config.toml`: the one setting outside the `Settings` seam.
- **The inspector renders the engine's `StreamDigest`, never a second read of the file.** `audio.md`
  has the sampling; `resonate-ui` does not depend on `resonate-codec`. Order: signal path as three
  `stage` cards (*Source*: codec in the lossless colour over depth, rate, channels, container;
  *Processing*: what the output mode did, gain applied; *Output*: mode in its colour over what was
  negotiated and the sink), detail cards, bitrate graph, tags. Follows the *playing* file only
  (inspecting a selected track = `probe_stream` from the window). The live profile restarts on every
  rebind (seek, sink switch, renegotiation): a `ProfileBuilder`'s windows are sequential and
  unresumable mid-span, so the graph covers the decoded span since the last rebind
  (`StreamDigest::profiled_from`). Digest rebuilt once per closed window (once per second of
  decoded audio); the decoder runs ahead of the sink by the ring's depth, so the graph leads what is
  heard. A line, not bars: `plot::stroke` polyline over a `plot::wash` from the accent to nothing
  (the reading is the rise and fall between windows; a `div` per column cost a layout node per
  point). Condensed at the digest's rate, not the frame rate: `PlayerModel::condensed` holds the
  reduced series, dropped when the poll swaps the digest for a different `Arc`, because
  `StreamProfile::series` runs to `MAX_WINDOWS` (a whole day) and reducing to `GRAPH_COLUMNS`
  points sixty times a second is a walk and allocation a frame for a picture changing once a second.
  A one-window profile is a level line.
- **The visualiser draws what the tap says is being heard; its arithmetic has no gpui.**
  `Pane::Visualiser` sits under `Section::Playing` beside lyrics and inspector (same six edits as
  the panes before it), follows the playing track alone: nothing playing = `kit::empty` under
  `Icon::Visualiser` (silhouette of bars under held peaks); a DoP stream = `kit::empty` saying it
  carries markers, not samples. Heading = lyrics pane's (title, artist via `opens`) with a
  `kit::figure` reading back the analysis (*4096-point · 48 kHz*), the hint, and a `kit::segmented`
  *Spectrum*/*Scope* living on the entity for the run. `spectrum.rs` is the arithmetic, tested as
  `curve.rs` is: hand-written radix-2 real transform (half-length complex over bit-reversed even
  and odd samples, split back to the real spectrum) under a periodic Hann window, scaled so a
  full-scale sine on a bin reads 0 dBFS; sixth-octave bands from the equaliser's
  `RESPONSE_FROM_HZ` to what the rate carries (`spectrum::top_of`: Nyquist, never under
  `RESPONSE_TO_HZ`, so a 96 kHz stream draws to 48 kHz), placed by `spectrum::across` on that axis
  and labelled at each decade under it (`spectrum::marked`, drawn by the curve's `marked_at`); a band narrower than a bin read at its centre
  between the bins either side, one past Nyquist left on the floor. Band = its loudest bin tilted up
  `TILT_DB_PER_OCTAVE` (3) about 1 kHz (pink noise stands level; a mastered record does not slope
  into the treble), drawn between `FLOOR_DB` (−78) and 0 over faint lines every 12 dB (no figures:
  a tilted reading is dBFS only at the pivot). A bar rises at once, falls at `FALLS_DB_PER_SECOND`
  (40); its peak is held `PEAK_HELD_FOR` (700 ms) before following (`PEAK_FALLS_DB_PER_SECOND`,
  20); all wall-clock, so a late frame falls further, not slower. Window sized by rate, about 85 ms
  (`ANALYSED_FOR`): 4 096 points at 44.1 and 48 kHz, up to 32 768 at 384. Scope = 20 ms of left over
  right, from the first rising zero crossing of their mid in the first span of a window twice that
  long (a steady tone stands still).
- **The visualiser's third view is the stereo picture.** *Stereo* (`Showing::Stereo`) reads the
  spectrum's window of left and right (about 85 ms) into `stereo.rs`, gpui-free and tested as
  `spectrum.rs` is: each channel's mean square in dBFS folded into the spectrum's `Bar` (rises at
  once, falls at its rate) and its highest sample into another, whose held peak is the meter's
  mark and the figure over the plot, and the channels' correlation (Pearson's,
  `None` over silence) settled towards each reading over `CORRELATION_SETTLES_OVER` (300 ms). The
  canvas draws a goniometer (side across, mid up: `sides_and_mids`, at most `DOTS_AT_MOST` dots, the
  mid, side and both channels' axes as guides) in the largest square the plot leaves, two level
  meters at its right, and the correlation as a mark on a −1..+1 track under it, in the failure
  colour below 0; the held levels and the correlation are figures over the plot's corner, each
  explained behind the pointer. Proved drawn against a nearly-mono tone in a virtual KWin.
- **The pane follows the display's clock while it has sound to draw.** A zero-size canvas asks
  `Window::request_animation_frame` whenever the transport plays and the engine hands the pane a
  tap (as the lyrics pane follows a synced set): bars and scope move once per display frame (120 a
  second on 120 Hz), not on the 16 ms poll, which beat against it. Once refused because a
  pane-asked frame re-ran the whole window (first cut: this machine's 240 Hz window ~6.5 % of a
  core to 32.5 %); cached regions made it affordable: a notified view marks only its own `Part` and
  the root's shell dirty. Measured (16-bit 44.1 kHz FLAC into a 48 kHz null sink, headless KWin,
  scratch catalog): 7.3 % of a core with the pane in front. Paused or nothing tapped: asks for
  nothing (bars still falling ask again on a timer at the poll's interval; a paused transport holds
  the paused moment). Being an entity keeps its state (transform tables, bars, three sample
  buffers) out of `RootView`. Every frame is still a whole-window GPU paint, gpui's, not the
  pane's. `RootView::new` calls `PlayerModel::listen_in(true)`: the engine taps from the window's first
  play, so the plot opens on what is already heard rather than empty for the ring's depth (cost
  in `audio.md`).
- **The analysis pane draws the whole playing track and says whether it is what it claims.**
  `Pane::Analysis`: fourth pane under `Section::Playing`, same six edits, `Icon::Analysis` (a
  waveform over its axis). Heading = visualiser's (title, artist via `opens`) reading back codec,
  declared depth and rate, or how far the decode has got. Under it: verdict in its colour over one
  sentence per finding (`Finding::told`); waveform (lane per channel, min-max polygon washed in the
  accent with RMS solid inside, heard part shaded, playhead a press seeks from); spectrogram
  (painted raster stretched to the card, frequencies as backed labels, cutoff a line in the
  verdict's colour); average spectrum (washed polyline over faint lines every 20 dB); levels and
  source as `fields` cards dealt by `beside`, lent by the inspector; recognition card, led to the
  top where the audio is not the song the file names, carrying *Take this name* there.
  `analysis_plot.rs` is the arithmetic, no gpui. Online's *Recognition* group is the `acoustid-key`
  field, written through the `Settings` seam as *Contact* is, read from the next start.
  `analysis.md` has the model.
- **Listen is a sheet over whatever pane is in front, not a pane.** It names a song that need not
  be in the library, so it belongs to no section: `ctrl-l` (bound in `answering_anywhere`) and the
  header ear (stopping its press like every header control) open it via
  `RootView::open_the_listener`, starting a recording at once; escape or a press outside the card
  closes it and stops one in flight. Stopping, in both stages, drops the task that would ask about
  the clip and puts the sheet back at `Idle` (a song named after Stop or closing is neither told to
  the desktop nor drawn). `listening.rs` = model, `views/listen.rs` = drawing. `ListenModel` holds
  the `Listens` the binary handed in and walks one `Stage`: `Recording` (the `Hearing` the bar draws
  from, the source recorded, which try it is), `Asking`, then `Found`, `Unknown`, `Silent`,
  `CaptureFailed`, `Unreached`, `Refused`, `Offline`, `NoService`. **A miss is listened to again**:
  a clip no service named, or one nothing reached, is recorded again up to `TRIES_ON_A_MISS` (3)
  before the sheet says so (*Nothing named yet, so listening … again (2 of 3)…*); a service out of
  reach or refusing is said at once. **Keep listening follows the music**: a switch under the
  sources (`keep_listening`, kept for the run) has every named song, and a last miss, followed by
  another clip `FOLLOWED_EVERY` (20 s) later, on and on (`Following`: `Waiting`, `Listening`, said
  in a faint line under the stage) while the card keeps the song named; a song naming the same
  title and artist (folded) is kept quietly, a new one is told to the desktop and heads the list,
  a miss or failure while following changes nothing. A press, Stop or closing the sheet ends the
  following. `then` is the whole decision, pure
  (`a_miss_or_a_silent_clip_is_listened_to_again_until_the_tries_run_out`,
  `following_tells_a_new_song_and_keeps_quiet_about_the_same_one_or_a_miss`). `stage_after` picks the landing for an error; a
  failed capture and a clip a service turned down have words of their own, not *nothing reached the
  recording* / *could not be reached*
  (`a_service_that_refused_the_clip_is_not_one_that_could_not_be_reached`). Recording and asking run
  on the background executor (the frame never waits on PipeWire or a network). `listen` takes
  whether Online is on *now*, from `ResonateApp::online` via `RootView::listen_now`: off =
  `Offline`, nothing recorded (Online switched off mid-run stops Listen reaching Shazam though its
  recognisers were built at start); on with no recogniser = `NoService`, saying they come at the
  next start (Online was off when the run began, or no network in the build), not claiming Online is
  off (`online_switched_on_in_this_run_is_not_said_to_be_off`). The recording note names the source
  in the stage, not the one chosen; source segments and microphone chips take no press while
  `is_listening`, and `listen_from` refuses one too, so a mid-recording press neither changes the
  next source nor misnames this one
  (`a_source_pressed_while_recording_is_refused_and_the_recording_names_its_own`). A found song is a
  card: cover at `theme::listen_cover()`, title, artist, album, year, naming service, *Listen
  again*, *Open* where the service gave a link, *Find it* (`search_instead` +
  `choose_pane(Pane::Tracks)` over title and artist: a held track is one press away; otherwise the
  search lists what the catalog and MusicBrainz know). Those buttons, and *Listen* under an idle,
  unknown, silent, failed or unreached prompt, are drawn in `sheet_actions`, a card-wide row from
  the prompt's left edge (`kit::actions` is a heading's row pushed right: in the sheet's column it
  stood the button off at the right of a line prompt and bar both begin at the left). The last
  `HEARD_KEPT` (8) songs stay listed for the run. *Desktop*/*Microphone* = a `kit::segmented` over
  chips of the microphones PipeWire offers (listed each time the sheet opens), written back as
  `listen-from` through the `Settings` seam. Online's *Listening* group writes the same key and
  `listen-for`; its *Recognition* group has the AudD token beside the AcoustID key, each a `Field`
  read at the next start. Its *ListenBrainz* group is the `listenbrainz-token` field, not read at
  next start: the binary's submitter follows the settings file, so a token given or cleared there is
  what the next submission (within half a minute) carries. With a token typed, *Send earlier
  plays* is armed by the first press (`telling_the_earlier_plays`, lowered with the pane's other
  armed presses) and asks `Library::tell_earlier_listens` on the second
  (`the_earlier_plays_are_asked_for_by_the_second_press_alone`); the submitter does the sending.
  **A token given is asked about at once.** Where Online is on and the build handed `Lookups::scrobblers` (the `Scrobblers` seam
  turning a token into a `Scrobbler`; `None` without `online`), `check_the_listenbrainz_token` asks
  `token_held` on the background executor and says in a toast whose token it is or that the service
  does not know it (a short-pasted token is found out then, not by listens never arriving).
- **Play next and Add to queue are row actions; Shuffle is the tracks heading's second play
  action.** Each track row, the playlists index row and each opened-playlist row carry the queue
  pair (on a playlist row they take the whole reach the row is in). Tracks heading: Shuffle beside
  Play; an opened playlist's heading too, starting at a time-chosen row and enabling queue shuffle.
  The + on a queue row and each opened-playlist row opens the playlist picker; index and
  opened-playlist headings use + to enter the target's track-browsing mode. Albums and artists
  panes carry neither (a row there scopes the tracks pane), nor does the queue pane (rows already
  in it). The only queueing gestures, each named once in `menu::PLAY_NEXT`/`menu::ADD_TO_QUEUE`:
  *Play next* = `Placement::Next` (straight after the playing track); *Add to queue* =
  `Placement::Queued` (after whatever is queued, ahead of the rest of the album or playlist
  playing). Album page, artist page and opened suggestion do not carry the pair (row is under the
  text); a suggestion's card queues with a mark. The queue pane marks a row waiting to play next
  with the *queue next* icon where its number would be, off `Queued::next`; its heading counts them.
- **The queue is drawn in the engine's four parts, each under a heading.** Published list =
  `order[..after] ++ next ++ order[after..]`, so a row's place relative to the one heard is all it
  is: `QueueParts::of` reads length, `PlayerState::queue_position` and `Queued::next` into runs of
  `Part::Heard`, `Playing`, `Next`, `Rest` (*HISTORY*, *NOW PLAYING*, *PLAYING NEXT*, *CONTINUE
  PLAYING*, the last naming the playlist where `Library::playing_playlist` still badges one). A
  queued row being heard is `Playing`, not `Next` (`Queued::next` still spans it). A heading is one
  more `uniform_list` row (eyebrow, count, hairline) so the list keeps one height a row; a one-part
  queue draws none. Heard rows draw at `HEARD_FADED` (0.55), lit whole under the pointer. Gestures
  still speak in queue rows; `QueueParts::line_of` and `line` are the one mapping to the list's
  lines, which `show_row` scrolls through (following the playing row, reach keys and type-ahead land
  on the row, not one heading short: `every_row_is_shown_at_the_line_that_draws_it`). A row dragged
  across a heading is sorted by the engine as any drop is, by the row it lands after.
- **Every major listing is ordered from a chip row and a header.** Tracks, albums, artists panes
  each hold an order and a reading on `LibraryModel::sorting`, kept for the run like the settings
  category and playlists order. Two writers: a sort icon opening the ORDER and READS chips, and the
  pressable column header (press names the order; a second on the one in force reverses it; the
  column in force wears the accent and a chevron). `views/sorting.rs` holds both: `Ordering` is the
  trait the five order enums implement, `sorting::order_row` the one chip-row builder (replacing
  three near-identical ones in `views/playlists.rs`). `listing::columns` takes a `Sorted` descriptor
  (columns offered, which is in force, a `fn` pointer for the press), so tracks pane, queue and an
  opened playlist draw one header and cannot disagree about a width while offering different
  vocabularies. A right press on a header offers every order it has.
- **The queue is ordered as a gesture, not a standing order.** Its heading's sort icon sends one
  `Command::Order`: the window builds a permutation over the rows it draws (scanned, or read via
  `Player::media`); the engine rewrites the play order in one pass, cursor re-seated onto where the
  playing row went, so a mid-track sort reopens no stream. A queue row is dragged and reached as a
  playlist row is; what comes next is still shuffle's and the transport's to say. The heading shows
  count, total length, row in play; *Clear* sends one `Command::Remove` over the whole span. The
  reach lives for the run, escape clears it, one left past the end of a shorter list is pulled back
  to the last row, not dropped.
- **A heading wraps rather than grinding its name down; `kit::actions` is the whole of how.** Every
  heading's controls sit in one (`flex_wrap`, `max_w_full`, pushed right), so the longest (opened
  playlist: Play, Shuffle, *Edit search*, +, *Drop shown*, Tidy, Sort, the pin, the ⋯ menu (Play,
  Play next, Add to queue, Rename, Duplicate, Export, Pin, Discard), Undo, Redo) flows onto a second
  line as the window narrows instead of clipping Play off the edge. The name keeps
  `theme::heading_name()` of room (`min_w`) whatever is beside it, so controls wrap, not the name
  down to a letter. Album and artist pages are out of this: their actions sit under the name.
- **Undo and Redo are drawn in the playlists headings only.** `RootView::undo_edit`/`redo_edit`;
  Redo only while a step is there to put back; each says how many steps stand behind the one it
  offers (`Undoable::behind`, the only number either stack publishes). Rows put in a playlist from
  the tracks or queue pane have no control to press, so the notice names `ctrl-z` beside where they
  landed; a press either way leaves a notice saying what was put back or done again, not what the
  playlist now holds.
- **A narrowed playlist says what a gesture reaches.** The heading's `NARROWED_HINT`: Play, Play
  next, Add to queue, Drop shown take the rows shown; Sort, Tidy, Export take the playlist whole.
  Tidy removes missing and repeated rows together in one undoable edit (no separate fold control).
  The playlist title opens rename, its selector appearing on hover. The hint is behind the pointer
  like every hint, and a search matching nothing draws no mark to hover. The sidebar's count is the
  narrowed one: standing in a playlist whose name the search misses reads 0.
- **A pane hands its rows out by reference count, not cloned per frame.** An opened playlist's rows
  are an `Arc<[PlaylistEntry]>` shared by model, pane, heading, list closures and every visible
  row; the reach is read off it once a frame as a `Reaching` (span + the cuts it holds), which a
  row in the reach takes by reference count too. `Held` carries an `Arc<[Cut]>`, so the picker's
  rows are copied only where the press hands them to the library. `LibraryModel` holds albums,
  artists, tracks as `Arc<[_]>`; `albums()`, `artists()`, `tracks()` hand back a pointer (the tracks
  pane used to deep-clone ~2 000 `Track`s, each a `PathBuf` and two `String`s, on each of sixty
  frames a second); the queue pane reads the same way. A whole list is walked only where a press
  asks: playlist menu's *Duplicate*, the index row's +, the queue's *Save as a playlist*, the tracks
  heading's *Add to playlist*, *Play next*, *Add to queue*, *Play* each walk inside the listener,
  not every frame that may never carry the press (fresher as well as cheaper).
- **The playing row is indexed, not searched for, and resolved once per pane.**
  `PlayerState::queue_position` names the row, so `RootView::queued_row` indexes the published
  queue and weighs the id found against `state.current` rather than walking. One `Playing` carries
  all the playback bar draws (title, artist, album, codec, a `Cover` of album id and
  picture-source file): panel, cover cell and signal path are one resolution, not three walks and
  two `Track` clones.
- **A playlist edit re-reads the playlists and nothing else; so do a scan and a counted play.**
  `LibraryModel::reload_playlists` takes the `ThePlaylists` read above; `reload` re-reads the opened
  playlist beside the listing (a scan runs it every `POLLS_PER_RELOAD` (20) polls and again at the
  end; a counted play once), so `plays:` and `played:` queries move under the window as `added:`
  does. Inside that read the listing is whole, so a rename re-reads the entries beside it: counts
  and total length are one grouped pass over the entries, not two subqueries a row, and the opened
  playlist is read once more beside them (a narrowed index otherwise loses the row the heading
  draws its name, count and kept order from).
- **A second process's edit is seen once its writes settle.** `watch_for_writes_elsewhere` reads
  `Library::written_elsewhere` (the writer connection's `PRAGMA data_version`, typed as a
  `WrittenElsewhere` that is only compared) on a background thread every `WATCHED_EVERY` (2 s) and
  reloads once the stamp moved since the last reload *and* read the same the tick before. A playlist
  or favourite `resonate mcp` changed, or a `resonate scan` beside the window, is drawn a few
  seconds after it lands; a scan committing batch after batch costs one reload when it pauses, not
  one a tick. This process's own writes never move the stamp, so its own edits are the reloads they
  always were. A busy writer answers nothing; the tick is passed over.
- **`Raise` activates the window; `Quit` closes it.** The binary's host answers `can_raise` with
  whether it holds a raise channel (the window's does; windowless `resonate play` and the bus's
  default `Host` answer false); `Raise` sends on it, `raise_when_asked` drains it on the gpui
  foreground and calls `activate_window` on every window (a Wayland compositor may still refuse
  without an activation token). `Quit` sends on the channel `resonate play` uses, drained by the
  gpui foreground on the frame timer.
