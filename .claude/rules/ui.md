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

- **Title names what plays.** `RootView::name_the_window` sets *Echoes — Pink Floyd · Resonate*
  (artist omitted if blank), else *Resonate*; only when the text moved
  (`the_window_is_titled_by_what_plays_and_by_the_app_where_nothing_does`).
- **The window is its own titlebar.** `WindowDecorations::Client`; the header draws
  minimise/maximise/close, drags the window, opens the compositor's window menu; under
  `Decorations::Server` neither controls nor drag handlers render.
  - Header: `theme::header_height()`, three columns; wordmark and controls each take sidebar width
    less padding, so the search field (at most `theme::search_width()`) centres on the window.
    Control = `theme::WINDOW_CONTROL` (31, odd so the mark centres on a whole pixel) square.
    *Resonate* drops `WORDMARK_CAPITALS_CENTRED_BY` to centre capitals on the accent square.
  - `views/chrome.rs` owns the frame: `theme::RESIZE_BORDER` gutter outside the painted window
    (resize edges, shadow); rounded corners on non-tiled edges. **Shadow is cast by a rim**:
    `shadow_rim` casts the `BoxShadow` from four strips under the content along its edges (gpui's
    whole-box shadow cost a quarter of a tracks-pane frame's GPU time). **Gutter repaints only where
    the resize edge under the pointer changes** (`resize_cursor`, in paint).
  - **gpui clips a child to its parent's rectangle, not its rounding**: `chrome::rounded` = which
    corners are round; `rounded_within_the_frame` = same less `FRAME_BORDER`, used by header (top)
    and playback bar (bottom).
  - **An edge held to the screen is not a resize edge.** `grabbed_edge` weighs only edges
    `held_edges` leaves free: none while maximised/fullscreen, whatever tiling the compositor
    reported (else a press on a maximised playback bar's bottom strip became a resize grab that ate
    the click; `chrome.rs` tests).
  - **Which of minimise/maximise draw weighs two readings**: `Window::window_controls` (compositor
    offers) and `ResonateApp::window_buttons` (`minimise-button`/`maximise-button`). A setting
    removes a button, never conjures one the compositor withheld. Close is not a setting; a double
    press on the bar still zooms. `window_min_size` = where the header's children stop clipping.
  - **All four marks are icons, not glyphs** (`Icon::WindowMinimise`, `WindowMaximise`,
    `WindowRestore`, `WindowClose`), strokes one device pixel at drawn size, lit via
    `icons::lit_on_hover`, all `theme::WINDOW_MARK` less overlap across.
- **Closing removes the window; quitting follows, never the reverse.** Close control, `ctrl-q` and
  the bus's `Quit` call `Window::remove_window`; `app.rs` quits once the last window closed
  (`App::quit` under a live window logs *window not found*).
- **The sidebar scrolls its middle and keeps its foot.** `RootView::sidebar` holds the tabs in an
  `overflow_y_scroll` region tracked by `sidebar_scroll`, with a `Scrollbars` bar over its edge; the
  download and enrichment status and *Settings* stand below it, never scrolled, so a window squeezed
  vertically scrolls the tabs and leaves the foot above the playback bar.
- **A menu's *Play* does what a click or `enter` on its cell does.** A track row's menu plays the
  listing from it (`RootView::play_the_row`, shared with the click); an album or artist cell's
  menu queues the album or artist whole (`whole_of`: album then track order, no search text, as a drag
  does); a playlist's menu *Play* sends `plays_in_order` first like every other playlist *Play*.
- **A playlist shown reads its own kept order.** `show_playlist` sets `keeping`, `row_order` and
  `row_reading` from the playlist's `kept` (defaults where none) when it changes the playlist opened,
  so a header press on one playlist never inherits another's.
- **A typed folder is looked at off the window's thread.** `root_typed` asks `is_dir` on the
  background executor (`looking_at_the_typed_root`), and a picked folder is canonicalised there too
  before `set_the_music_folder` or `name_the_vault` see it.
- **The window names itself to the compositor twice.** `WindowOptions::app_id` = `APP_ID`, matched
  against `resonate.desktop` (its `StartupWMClass` held to it by a test in `app.rs`); the caption is
  `Window::set_window_title` on open (gpui's Wayland backend never reads `WindowParams::titlebar`).
- **Everything clickable in the header stops the press**: `chrome::titlebar` puts
  `start_window_move` on the whole bar, so controls and the search field carry
  `on_mouse_down(Left, stop_propagation)`.
- **The search field is a real text input.** `views/field.rs` owns `Field`; `edit.rs` the gpui-free
  `Edit` (content, caret, selection), tested windowless. A word is a run of graphemes of one class
  (space, letters/digits, symbol), letter runs broken at UAX #29 boundaries (`Edit::runs`). The view
  is a custom `Element` with an `ElementInputHandler` (IME); a selection drag is followed from a
  `Window::on_mouse_event` registered in `paint`.
  - **A preedit is held to what the field holds.** `replace_and_mark_text_in_range` runs the text
    through `one_line` as a commit does, and the mark is the range `Edit::replace` reports it filled,
    so a preedit cut at `LONGEST_CONTENT` marks only what stayed.
  - **A click lands on a grapheme boundary**: `Edit::move_to` and `select_to` snap an offset to the
    nearer edge of the cluster it falls in (`Edit::snapped`; keyboard motions already step by
    cluster).
  - **A selection is a quad per visual run**: `spread_of` walks the shaped line's glyphs (position
    and logical `index`) and joins the cells whose index the selection holds that touch, so a range
    over mixed-direction text paints one quad per run, not one from its first glyph to its last. The
    caret and `closest_index_for_x` are still left-to-right arithmetic.
  - **An input method composing with no field focused starts a search.** The window's root focus
    registers a `Composing` input handler (a canvas, `composition_watch`) that hands a preedit to the
    search field: focuses it, puts the caret at the end and marks the preedit there; the commit
    follows once a preedit is held. Text arriving with no preedit marked is ignored, because the
    keys the window already heard (`typed`) reach an input handler too and would be typed twice. It
    takes no preedit while something stands over the pane. Only the root's own focus is covered: a
    ringed control holding the caret has no handler, so a dead key pressed there composes nothing
    (`an_input_method_composing_at_the_window_begins_a_search_and_its_commit_lands_in_it`).
- **TIDAL is signed in to from its group, never by pasting a token.** `RootView`'s `SigningIn`:
  `TidalSigning::Idle`/`Asking`/`Waiting`, a stop flag, the task; `sign_in_to_tidal` runs the
  `SignsIn` seam's two calls on the background executor; the code shows as a `kit::figure` beside
  *Open the page* and *Stop*. The token lands in the refresh-token field, the global and the file at
  once; a refusal is a toast naming which (`providers.md`). hifi-api and Monochrome addresses are
  optional overrides: blank = the hosted service where *Ask the hosted services* is on, else
  nothing.
  - **An account given is asked at once.** Every field of the Subsonic and TIDAL groups writes the
    global `Online` as it writes the file (`Sourcing::providers`). `the_sources_moved` calls
    `LibraryModel::sources_moved`: every *No provider is set up* download returns to *Queued* and
    every unheld want is asked, as *Poll now* does; turning Online on does the same
    (`a_song_asked_for_before_any_provider_was_set_up_is_fetched_once_one_is`).
  - A server address must begin `http://` or `https://` with a host (`reads_as_a_server`); else a
    toast, nothing stored.
- **Last.fm is signed in to from its group; the password is typed, sent once and never kept.**
  `views/settings/lastfm.rs` holds four `Field`s (`LastfmAccount`): API key and secret kept as
  `lastfm-key`/`lastfm-secret`, user name and password kept nowhere. Enter or *Sign in to Last.fm*
  asks `Scrobblers::signed_in_to_lastfm` on the background executor; the password is emptied either
  way, a session is written as `lastfm-session`; a refusal says what could be wrong, anything else
  that Last.fm was not reached (`last_fm_is_signed_in_to_from_its_group_and_the_password_is_never_kept`).
- **A secret is drawn as marks.** `Field::masked` (Subsonic password, TIDAL secret and token,
  AcoustID/AudD/ListenBrainz keys, Last.fm key, secret, password) shapes one `•` per letter; `Shown`
  maps every offset both ways; copies and cuts nothing.
- **The caret blinks as the desktop says.** `CaretBlink` on `Stored` and `ResonateApp` (`as_built`
  500 ms; `steady` holds it lit); every field's blink task rereads it each tick. The binary's
  `caret_as_the_desktop_blinks` asks on a `resonate-caret` thread via
  `resonate_mpris::caret_blinking` (Settings portal, one-second deadline); no answer keeps the half
  second.
- **Search undo steps are spans of edits, not keystrokes.** `edit.rs` keeps a bounded stack of
  snapshots (`UNDO_DEPTH` 128, content at most `LONGEST_CONTENT` 16 KiB, cut at a character
  boundary), coalescing consecutive edits of the same `edit::Span` (`Typing`, `Removing`,
  `Composing`; not `resonate-core::Span`) *and* resuming from the selection the last left. Paste, cut,
  clear are `Span::Discrete`; whitespace closes a typing run. **A key typed arrives through
  `replace_text_in_range` and is `Span::Typing` (`Edit::type_over`), so it resumes only from the
  selection the last left and a letter typed over `ctrl-a` is a step of its own; only a replacement
  while a preedit is marked is `Span::Composing`, and a preedit begun with nothing marked starts a
  step (`Edit::end_the_composing`), as the commit ends one.** `Field::open_with` loads a field another
  owner reuses (the equaliser's figure) through `Edit::start_over`, so undo cannot reach the text of
  the last use. **An edit changing nothing is no step** (`Edit::edited`). History lives only as long as the run; the three keys bind under `SEARCH_CONTEXT`;
  outside the field they walk a playlist edit.
- **The window names itself; a negated binding is dead without it.** Root div carries
  `key_context(WINDOW_CONTEXT)`: stack `["Resonate"]` at rest, `["Resonate", "Search"]` in a field.
  gpui's `depth_of` returns `false` for an *empty* slice before the `Not` arm, so an unnamed window
  disables every `!Search` binding. `app.rs`'s tests pin both stacks and that gpui still refuses the
  empty slice.
- **An opened album/artist stands under its category; pressing the category again leaves it.**
  `RootView::in_front` (`in_front_of`) = Albums for an album, Artists for an artist, else the pane.
  `choose_pane` (`landing` is the windowless decision): the row already in front returns to its whole
  category (clearing the scope or closing a playlist); a category whose page stands behind another
  pane returns to it; *Tracks* always lists every track; else only the pane changes.
- **`ctrl-tab` walks the sidebar; browse panes answer the reach keys.** `NextPane`/`PreviousPane`
  step `Pane::BROWSE` via `choose_pane`, wrapping (`stepped_pane`). `Shift::Listing` names a browse
  listing by `Listed` (a `UniformListScrollHandle` so `show_row` centres); a listing refuses what it
  has no order of its own to change (`Shift::is_edited`: `delete`, `alt-up`, `alt-down` dead there).
  `ctrl-shift-left`/`right` step the queue wherever the caret is not in a field (in one,
  `SelectWordLeft`/`Right` outrank them).
- **A search remembers the scroll it began over.** `set_query` records the browse pane's top row
  when the query leaves empty (`left_for_the_search`) and, when it returns to empty on the same pane,
  hands it to `landing_on`, so clearing a search lands the list where it was.
- **A search is left for its results from the keyboard.** In the search box `down`, `tab`, `enter` all
  land on `RootView::go_to_the_results`: caret returns to the window, the first listed row is
  reached. A read the typing is still settling is hurried (`read_the_search_now`) and the reach waits
  for it (`waiting_for_the_results`, cleared by the next keystroke), so `enter` straight after the
  last letter lands on the new results, not on none or the old ones. `tab` binds under
  `SEARCH_CONTEXT` to outrank the window's; from any other field `tab_onward` is `focus_next`. A moved query drops a reach left in a listing
  (`down_and_tab_in_a_field_are_the_fields_own_before_they_are_the_windows`).
  - `Listed::Albums` steps in reading order (a page = whole grid rows); the reached cell wears
    `reached_ring`.
  - *Top results* is one run, `Listed::Top`, in drawn order: held songs (`SONGS_AT_THE_TOP`), songs
    found beyond the library (`FOUND_AT_THE_TOP`), held artists (`STRIP_AT_MOST`), artists not held,
    held albums, albums not held. `up`/`down` move by drawn line: a song is a line, a strip is one
    line whatever its cells, so `down` leaves a strip for the next section; `left`/`right` step the
    cells of a reached strip (`TopRun::along_the_strip`) and seek everywhere else.
    `search::TopRun` is the arithmetic; `RootView::press_at_the_top` presses what `enter` reaches as a click would; `show_row` sets `reached_unseen` and
    `kit::brought_into_view_within` scrolls page and strip just far enough
    (`the_artists_songs_and_albums_on_the_top_results_are_one_run_the_keys_walk_and_press`).
  - `Listed::Favourites`, `Missing`, `Suggested` carry the reach into those rows (`enter` plays a
    track; on a Missing row opens the album or artist); Missing's two tabs let go of the reach on
    switching (`let_go_of_the_reach_in`). `Listed::Offered`/`Heard` reach the non-`uniform_list`
    panes (Suggestions' cards in `in_shelf_order`, Statistics' three tables as one run): `show_row`
    sets `reached_unseen`, the element carries `kit::brought_into_view`, a canvas that moves the
    pane's `ScrollHandle` on next prepaint and spends the flag.
- **Media keys are a fallback, not the feature.** A desktop grabbing transport keys calls MPRIS; where
  none is grabbed the key reaches the window. gpui names an unmapped keysym by lowercasing
  `xkb_keysym_get_name`: `xf86audioplay`, `xf86audiopause`, `xf86audiostop`, `xf86audionext`,
  `xf86audioprev`; `XF86Back`/`Forward` map to `back`/`forward`. All seven are in
  `answering_anywhere`; `app.rs`'s test pins the names against `Keystroke::parse` (an unreadable name
  binds to nothing, silently).
- **Typing at a list no search narrows jumps through it.** The search narrows tracks, albums, artists,
  playlists-index and opened-playlist panes; the queue narrows on nothing, so `RootView::typed`
  routes a keystroke there to `TypeAhead` first. `jumped` prefers a row *starting with* the letters,
  wraps; `RootView::jumping` is the one place saying which panes opt in. Pill says **JUMP TO** or
  **NO MATCH** in words, no id, no listeners; held `HELD_FOR` (1100 ms). `typing.rs` holds the
  gpui-free arithmetic; names fold through `resonate_library::folded_letters`.
  - **Names a jump reads come off the render thread.** `QueueNames` holds the queue's titles against
    a `NamedAt` (`Queued::revision`, `Library::names_stamp`, `Player::media_revision`);
    `names_in_the_queue` hands them out while all three stand (`CatalogStamp::still_holds_at`), a
    moved name or media revision asks again but meanwhile hands out what it holds. Asking runs on the
    background executor (`queued_rows`: one `Library::tracks_with_ids` pass in 500-id batches, then
    `Player::media`, then the stem); each name is held folded (`Named`). `named_again` reads only the
    rows the queue edit brought and those `Library::names_moved_since` says were retitled
    (`a_queue_edit_names_only_the_rows_it_brought`). A keystroke before they land still counts
    (`jump_where_typed`).
- **Backspace drops a typed letter before a reach, and never the row a jump landed on.** `jumped_to`
  notes the reach in `landed_by_a_jump`; while it stands (`stands_where_a_jump_landed`) backspace does
  nothing, even after the pill lapsed. Any hand-made reach (`reach_by_hand`, `reach_at`, a move, a
  drop) forgets it; `delete` takes a jumped-to row.
- **Typing anywhere searches; the letter that starts a search puts the caret in the field.**
  `RootView::typed` hands a letter no jump takes to `type_where_typing_goes`: focuses the search and
  appends it.
  - `space` at the window = `PlayPauseUnlessTyping`; `is_typing` makes it a typed space for `HELD_FOR`
    after the last letter or while a jump is live. The hardware play key keeps `TogglePlayPause`.
  - Stop, shuffle, repeat = `ctrl-s`, `ctrl-h`, `ctrl-r`: bare letters fired on every letter of a
    search (`typing_at_the_window_searches_with_every_letter_and_its_spaces_rather_than_steering`).
    `shift-right`/`left` seek `SEEK_FURTHER_SECONDS` (30; plain arrows five), `ctrl-m` =
    `toggle_mute`, `ctrl-u` = `toggle_queue`, volume `ctrl-up`/`ctrl-down`; plain arrows move lists.
  - An action fires before any `on_key_down` and stops propagation; a key bound with no predicate
    never reaches the query.
  - **Which predicate a key carries is structural**: `answering_anywhere` (none),
    `answering_away_from_a_field` (`!Search && !Control`), `answering_where_the_caret_is` (control
    context), `answering_on_a_band` (`BAND_CONTEXT`) are four lists; `bindings` concatenates them with
    `field::bindings`. A new key goes in one;
    `every_key_not_named_to_answer_anywhere_is_dead_while_a_caret_is_held` walks the second.
  - Escape blurs and clears, closing the naming row before clearing a search; a press outside blurs
    via `on_mouse_down_out`. `ctrl-f` takes focus deliberately (no predicate) and selects what is
    there.
- **Escape away from the field is a chain, and a notice is in it.** `LeaveSearch` binds under
  `SEARCH_CONTEXT` alone; `RootView::typed` hears escape elsewhere. Order, over-the-pane before
  beside-it: delete sheet (`keep_the_track`), Listen sheet, magnified cover, playlist picker
  (`stop_naming`), shown record, open menu, downloads panel, standing toast (`toast::dismiss`), live
  type-ahead (`stop_typing`), then `dismiss_search`: close the naming row, clear the query, or (query
  empty) `RootView::step_back`. A scoped search takes two presses, words then scope: typing is not a
  gesture on the scope (`LibraryModel::set_query` leaves `selection` alone); callers meaning the whole
  library (`revise_search`, `search_instead`) call `show_everything` first.
  - A focused field never reaches the match (`editing` returns first); `editing` asks
    `RootView::text_fields`, the one list of every `Field` the root holds
    (`what_is_typed_into_the_listenbrainz_token_stays_out_of_the_library_search`). Each field still
    needs its own `dismiss_search` arm (the AutoEq search's `leave_looking`;
    `escape_in_the_autoeq_search_clears_it_and_leaves_the_pane_where_it_was`).
- **What stands over the pane holds the pane's keys back.** A menu, playlist picker, magnified cover,
  record card, Listen sheet or delete sheet is `something_stands_over_the_pane`; meanwhile reach,
  page, widen, move, `enter`, `delete`, undo, redo and typed letters answer nothing. The check sits in
  each gesture, not the bindings; `reach_row` and `play_reached_rows` step and press the menu first.
  Escape is the one key still heard (`views/root.rs`'s driven tests hold each sheet).
- **A slider is grabbed on the press and followed from a window-wide surface, not the rail.**
  `views/slider.rs`: the press starts a `Grab`; `drag_surface` (an absolutely positioned *child* of the
  app, occluding while held) carries move/release listeners. It must be a child: `chrome::frame` adds
  its own `stop_propagation` move listener to the app element and one element's listeners run in
  reverse order. gpui reports motion only inside the window, so a drag past the edge freezes at the
  last inside value; `on_mouse_up_out` still hears the release.
  - Volume applies on every move (`VOLUME_SETTLE` debounces `config.toml` writes). A seek only previews
    and commits one `Command::Seek` on release, and only if the press's track still plays
    (`a_seek_rail_released_after_the_track_changed_seeks_nothing`); `PlayerModel::seek` holds the
    requested position until its `Outcome` answers (a refused seek restores the published position
    and raises the command toast); only `shown_position` reads the target, `state` stays what the
    engine published.
  - Wheel over the volume cluster = a `VOLUME_A_NOTCH` (touchpad pixels in `PIXELS_A_NOTCH`) while
    `ResonateApp::scroll_volume` says so; `RootView::volume_aimed` is what a run of notches or keys
    adds to (reading the published volume back lost all but one per poll).
  - **A press on the speaker, or `ctrl-m`, mutes; mute is not a volume.** `toggle_mute` sends
    `Volume::MUTE` and keeps the heard level in `muted_from`, storing nothing. `muted_at` reads the
    pair only while the engine publishes mute, so a volume set over the bus ends the mute. Where the
    device has the slider (`OutputStatus::device_turned`) the press is `Command::SetDeviceMute`
    (`audio.md`).
  - Release is taken from three places (inside the window, `on_mouse_up_out`, a move reporting no
    button held). The equaliser's handles ride the same surface: a `HeldBand` beside the `Grab`
    (`eq.md`).
- **`PlayerModel::state` returns a borrow of the model, not a copy.** Clone it before anything taking
  `&mut Context`: `let model = self.player.read(cx); let state = model.state().clone();`.

## The design system

- **`theme.rs` = palette and scale; `views/kit.rs` = the vocabulary every pane is built from.** A
  palette is a `Flavour`: surfaces `background`, `surface`, `raised`, `hover`; `border`, `outline`;
  `text`, `muted`, `faint`; `pitch`, `paper` (either may sit on an accent fill); `scrim`; `alarm`;
  seven `Accents`; `native` (the accent it was built around). Worn flavour, accent and `TextSize` =
  `resonate_core::Appearance` (in core because the config reader, built without `ui`, needs it). Its
  `None` accent is the palette's own, resolved only by `Flavour::accent`; `theme::wear` is the only
  setter.
  - Every colour and every text-holding measure is read through its own function
    (`theme::accent()`, `theme::row_height()`, `theme::text_base()`; measures are `scaled` off the worn
    `TextSize`): a theme change is one write behind one lock. Only compositor-owned values stay
    `const` (`CORNER_RADIUS`, `RESIZE_BORDER`, window marks, rail track and thumb, `WINDOW_MIN_*`).
  - **Ink on an accent is chosen by contrast**: `theme::ink_over` weighs `pitch` against `paper`; it
    takes a colour, not the worn accent (`kit::dot_swatch` checks an accent not yet worn).
  - Semantic colours come off the worn accents: `bit_perfect`, `lossless`, `done` green; `repacked`
    blue; `dithered` amber; `failure` red; `suspect` peach; `converted`, `lossy` muted grey. One
    colour, one meaning in every palette. `theme::tinted` makes a colour a wash; `theme::selection` is
    the accent worn thin.
- **Every theme is dark; contrast tests hold each readable.** Sixteen (`Theme::ALL`): Resonate's own
  warm near-black, Midnight, Graphite, AMOLED, Plum, Rosé Pine, Rosé Pine Moon, Catppuccin Mocha,
  Macchiato, Frappé, Nord, Gruvbox Dark, Tokyo Night, Dracula, One Dark, Solarized Dark. Each carries
  the same seven accents (`Accent::ALL`: Mauve, Blue, Teal, Green, Amber, Peach, Red) under its own
  hex.
  - **A fresh install wears the palette's own accent** (`native`; `Appearance::DEFAULT` names none);
    the Colour card offers it as a ringed swatch ahead of the seven; choosing none again =
    `Setting::Accent(None)`, removing the `accent` key. Theme cards are `theme_swatch()` wide, four a
    row in a `settings_column()` body.
  - **AMOLED's `background` and `surface` are pure black**; raised and hover still step up.
  - **Where a published palette names no hue for an accent, its terminal mapping decides** (Dracula's
    ANSI blue is its purple, magenta its pink). One Dark's body text is `d7dae0`, `abb2bf` muted;
    Solarized's text is `base2`, muted `base1`.
  - **Contrast bars** (`theme.rs` tests walk every theme x every accent): text 7:1 on `background`,
    `surface`; 4.5:1 on `raised`, `hover`; `muted` 4.5:1 on `background`, `surface`, `raised`; `faint`
    4.5:1 on the panes (it is text: count, key, year, placeholder); ink on an accent fill 4.5:1;
    accent 3:1 against `background`; inks step down `text` > `muted` > `faint`. Where a published
    palette's own step reaches a bar that step is quoted; elsewhere faint is lifted along its hue
    (`THIRD_INK`); Nord's aurora red is drawn a fifth of the way to `nord6`.
- **A palette is quoted or ramped; hand-writing twenty is how one drifts.** Quoted = a published
  palette's own hexes. Ramped = `ramp` over a `Recipe` (native accent, hue, chroma, alarm, `LitAt`)
  through a fixed lightness ladder, then all seven accents at the canonical hues. Midnight, Graphite,
  Plum are recipes. Flavours are `static LazyLock`s. `hsl` is pinned by
  `a_ramped_palette_reads_back_the_lightness_it_was_asked_for`. The three share the seven *hues* (an
  accent is what a colour *means*) but each lights them via its own `LitAt`
  (`each_ramped_palette_lights_the_accents_its_own_way`).
- **Two faces; figures always in the second.** Inter for text, JetBrains Mono for anything compared
  across rows (clocks, rates, counts, bitrates, tag values). `theme::mono` builds the `Font` with
  `tnum`; `kit::figure` is the one way to draw one. Neither face is embedded, so **the drawn family is
  settled once against the machine**: `fonts::settle` walks a ladder per face (Inter, Adwaita Sans,
  Cantarell, ...; JetBrains Mono, Adwaita Mono, ...) against `cx.text_system().all_font_names()` at
  start; neither ladder offers a family the other does (test); a machine with none keeps the wanted
  family. Not a setting; the About card reports the two (`fonts::drawn_in`). `theme::text_xs()`
  through `text_title()` and `text_lyric()` are the whole type scale, set via `text_size`.
- **A control is a `kit::button` in one of four tones; nothing draws its own.** `Tone::Primary` =
  accent fill, the one gesture a heading leads with (*Play*). `Tone::Outlined` = raised secondary
  (*Play next*/*Add to queue*, one pair one tone). `Tone::Destructive` = failure-edged, only for
  losing something for good. `Tone::Ghost` = everything else. `kit::icon_button` = the square
  version; `kit::chip` = pill an order, reading, cap or setting is chosen from; `kit::badge` = small
  mono tag; `kit::format_badge` = codec badge beside depth and rate from `format::quality` (*24-bit /
  96 kHz*, never `24/96`), coloured by `Codec::is_lossless`.
- **Pressability is the builder's decision; a greyed control carries no press at all.** `kit::Press` =
  `Takes` | `Greyed`; `kit::button_when` and `kit::mark_when` read it. `Greyed` draws at `kit::GREYED`,
  takes `cursor_default`, no hover style, pointer cursor, listener or focus ring: it must be decided
  where the element is built, as gpui's `hover` has `debug_assert!(hover_style.is_none())`.
  `settings::action` returns the bare greyed button on `busy`, else attaches listener and focus ring
  (`in_the_ring`).
  - **Adding and forgetting a folder are not greyed: they wait.** `LibraryModel::add_roots` and
    `forget_root` put the request in `roots_waiting` (`RootWaiting::ToAdd`/`ToForget`) wherever a
    `Work` runs; `take_up_what_waited` runs when a pass hands its `Work` back: a forget first, one at
    a time, then every waiting folder in one scan. A forget mark is keyed by its root, and **armed by a first press** like *Reset everything*:
    `forgetting_the_folder` names the root asked about (the mark says *Press again to forget*), a second
    press on the same root calls `forget_root`, any other press or leaving the pane lowers it
    (`lower_every_armed_press`).
  - **An enrichment is deliberately not a `Work`**: `LibraryModel::enriching` is a slot of its own,
    started with `for_the_pass` (`online.md`). A running lookup greys *Look up*, *Refresh all* and
    *Look for missing covers* via `held_back`; a scan can still start under one. `RootView::enrichment_status` sits
    above the sidebar's *Settings* row while `LibraryModel::is_enriching` (a globe in `theme::faint()`
    and *Enriching…*; a press opens `Category::Library`); the toast `looked_up` tells what was found.
- **What a listener reaches for is what the lookup asks next.** `LibraryModel` holds one `Sought` for
  the run and hands it to every `enrich`. Set via `LibraryModel::ask_about`: scoping the tracks pane
  to an album or artist, playing a row, and `ask_about_what_is_drawn` whenever a listing is read with
  an artist scoped. Only reorders what the pass has left: a nudge, not a command (`library.md`).
- **Every pane opens with the same heading; a page inside a category opens with a hero.**
  `kit::heading` holds a `kit::heading_row`: eyebrow naming the section, `kit::title`,
  `kit::subtitle` (counts, total length), `kit::actions` on the right; under it the *Reads* chips
  (`listing::reads`) and a naming row where a pane has one.
  - **The name keeps its room; actions drop under it.** Each name block holds `theme::heading_name()`
    at least and `kit::heading_row` wraps (`kit::actions`: `flex_wrap`, `max_w_full`, pushed right),
    so a long name is not ground to a letter and no action is pushed off.
  - **An album or artist is not a pane heading with a picture bolted on**: `album_page_heading` and
    `artist_page_heading` = `kit::way_back` top left and a `kit::hero` under it. Cover, portrait or
    opened suggestion's art is a square of the text column's own height at `Drawn::OnThePage`; column
    = eyebrow, `kit::hero_title` at `text_title`, lines about it, bottom-aligned. The column's height
    and width are read a frame behind by absolute canvases (`kit::measures_its_height` into
    `RootView::hero_height` via `hero_side`; `kit::measures_its_width` into `hero_width`, taken by
    `kit::hero_title` and `kit::wraps_within`, since what wraps must be told its width); until
    measured the cover uses `scope_cover` and the title is one line ending in an ellipsis.
  - Action row = the column's last child: Play, Shuffle (album, artist), favourite, a sort icon where
    the listing sorts. Shuffle begins at a time-chosen row and enables queue shuffle
    (`RootView::play_shuffled`). **Play plays in order**: every whole-list Play sends
    `SetShuffle(false)` via `RootView::plays_in_order` first. A row pressed to play from leaves shuffle
    as is and queues what Play would (`RootView::play_the_listing_from`: held rows where they are the
    whole listing, else `with_everything_listed`;
    `a_row_played_from_a_listing_read_in_part_queues_the_whole_listing_from_that_row`).
    `RootView::play` clears the playing-playlist mark that only `play_playlist` sets.
  - An album adds the info mark (and the download mark, *Get the rest* on its hint, where tracks are missing); an artist adds one where
    it has genres, and the Albums/Tracks choice at the row's right. Play next, Add to queue, Add to
    playlist are not on the page. The row does not wrap (taffy sizes a wrapping flex item at
    min-content width).
  - The opened playlist uses the same hero, cover = `views/mosaic.rs` art, beside it a faint line of
    when it was made and last played; Play and Shuffle; *Add songs*, *Sort*, *Tidy* for a list; the
    pin; `Icon::More` opening the index card's right-press menu. The title opens the shared rename
    field. `kit::way_back` reads *Playlists*, above eyebrow *PLAYLIST* or *SAVED SEARCH*.
  - **A title that is a link is `kit::linked_title`, never `kit::title` over an `opens`** (the link got
    no width). Such a link is cut before drawing, as gpui never truncates no-wrap text past its first
    measure: `RootView::playing_heading` (lyrics, visualiser, analysis headings) measures into
    `heading_room` a frame behind and cuts with `kit::cut_to_fit`; the inspector cuts at
    `inspected_room`; `by_line` cuts the artist at its room, the album taking the rest. The artist
    under a title is `kit::linked_subtitle` (`kit::subtitle` over an `opens` stretched the link across
    the column).
- **A track row is the same eight cells wherever drawn.** `listing::columns` = the header the tracks
  pane, queue and opened playlist put over their rows; `number_cell`, `title_cell`, `artist_cell`,
  `format_cell`, `heard`, `length_cell` the cells under it, so no two disagree about a width. Title and
  artist give way together (`title_room` and `artist_room` start from `theme::row_artist()` and both
  shrink, only the title growing). A track with other copies carries `+N` inside the title cell
  (`tagged_title_cell`); text cells end in an ellipsis. The playing row draws `listing::playing_mark`
  in the number cell and its title in the accent. Row controls sit in `browser::row_controls`,
  invisible until hovered.
  - **A listing narrower than its columns gives up HEARD, then FORMAT, before the names.**
    `listing::Shown::within` weighs the width the header measured into `RootView::columns_fit` (a
    `listing::Fitting`) against the fixed cells, `NAMES_AT_LEAST` and the controls; `columns`
    publishes the choice and every row reads it back; an unmeasured header draws every column
    (`a_narrowing_listing_gives_up_what_was_heard_then_the_format_before_the_names`).
- **Whatever a gesture takes out of the queue is kept; a toast says so.** `TakenOut` = what the last
  gesture took (rows, the row they started at, the id of the row before them); `drop_rows` writes it,
  so a row's ✕, a reach of thirty rows and backspace all keep what they take; *Clear* is the same
  call over `Span::between(0, last)`. `drop_rows` tells it as a toast (*Took 34 tracks out of the
  queue*); *Put back* stays in the heading and sends the rows as a `Command::Insert` at
  `Placement::At` the row they came out of. A menu's *Take out of the queue* on a track queued
  more than once takes every place in one `Command::RemoveAll` (`RootView::drop_queue_runs`): one
  toast for the lot, one step per run of adjacent rows, each put back where it stood. Kept as `QueueItem`s, not ids (`one_id_each` mints new
  ones). `Reach::after_dropping` lands the reach on the first row after the gap, the row now last, or
  nothing.
- **A run of gestures is walked back one at a time; nothing queued since takes the walk away.**
  `TakenBack` = the bounded stack (`KEPT_GESTURES` sixteen, oldest out first): *Put back* pops the top
  and inserts after the row it followed (`TakenOut::after`, found by `landing_in` wherever it now
  stands). A step stands while none of its rows is back in the queue (`stands_over` against a set of
  ids that `Unclaimed::claim` hands back to a row on its way in). *Take out again* walks forward:
  `TakenOut::standing_in` finds the rows by id as the run they were put back as and sends
  `Command::Remove`; a step whose rows no longer stand together is not offered; a fresh gesture clears
  the forward walk. **The walk has the undo keys while the queue is in front**: `undo_edit`/`redo_edit`
  try `put_the_queue_back`/`take_the_queue_out_again` first on `Pane::Queue`. The heading is drawn
  over an emptied queue while a *Put back* step stands, *Clear* and *Save as a playlist* hidden.
- **A card leaves out what the file does not declare; cards stack in two columns.** `fields` takes an
  `Option` per row and draws only what is there, one faint line where a whole card is empty; `beside`
  deals the five cards into two `flex_col` columns that wrap to one in a narrow window.
- **The inspector is the one pane reading the catalog, not the engine; the *Heard* card is why.** Every
  other card is the `StreamDigest` or `OutputStatus`; play count and last play come off the `Track`
  `Playing` already resolves through `LibraryModel::track_of` (no query). A row no scan has seen
  answers *no library row*, not zero. `format::since` reads dates as the largest span that fits.
- **What a search matched is lit where drawn, by one element.** `listing::matched` answers the text
  itself where nothing is lit, else a `StyledText` with `HighlightStyle` runs (a lit cell still
  truncates, wraps, measures as one string). Runs are `Search::lit` for that cell's `Column`
  (`library.md`), so a fold-reached name lights the spelling drawn. Only browse panes call it. A run
  is the accent at `FontWeight::MEDIUM`.
- **The albums pane is a grid whose column count follows the window in the frame it moves.** A
  `uniform_list` row is `columns` cells of `theme::grid_cover()`. Columns read `RootView::grid_width`,
  a `kit::GridWidth`: the canvas under the list (`kit::measures_the_grid`) writes on prepaint how much
  *narrower than the window* the grid is, and `RootView::region` tells it the window's width whenever
  the pane is built, so a resize draws at the new count the first frame
  (`a_grid_is_as_wide_as_the_window_now_less_what_it_was_narrower_by`). **The list is not built until
  that cell holds something** (the `max(1)` fallback stacks full-width cells); the canvas is
  `absolute` and sized by the container, so it measures the same with the list there or not. A cell's
  cover is `Drawn::InAGrid`.
- **An empty pane says what is missing and what to do** (`kit::empty`: the pane's icon in a ring, a
  sentence naming the state, a second naming the gesture that fills it). **A search that matched
  nothing offers the spelling the catalog holds, as a press**: `kit::empty_offering` adds a
  `Tone::Outlined` button *Did you mean Billie Eilish?* from `RootView::nothing_matched`, putting the
  text in the search field via `RootView::search_instead` (editable, not run behind the listener's
  back). It draws `LibraryModel::instead`, filled by `browsed` only where albums, artists, tracks and
  sung matches all came back empty; with no spelling but sung words it offers *Sung in N tracks* via
  `sung_offer`. The albums and artists panes build it before the heading (the model's borrow must end
  before `cx.listener`).

## Drawing

- **A plot is quads, never a path.** gpui 0.2.2 rasterises each vector-path batch through a
  window-sized 4× MSAA texture every frame, costlier on an integrated GPU than the plots. `views/plot.rs`
  uses batched `paint_quad`: `stroke` = quad over each segment's bounding box; `wash` = quad from each
  segment's middle height to a level; `between` = quad per column from the higher of two traces to the
  lower. `PathBuilder` has no caller.
- **An icon is an embedded SVG reached by an `Icon` variant, never a text glyph; one list is all an
  icon is.** `icons!` takes variant and file name and writes the enum, `Icon::ALL`, `Icon::path`,
  `Icon::drawing`, so a variant cannot be missing from the array the `AssetSource` walks.
  `crates/resonate-ui/assets/icons` = one silhouette per variant; `icons::Embedded` is the
  `AssetSource`. `Icon::Resonate` is the one icon with no file there: `icons::masked` derives it from
  `packaging/resonate.svg` (the `<g>` alone, gradient swapped for plain stroke, 24-unit `viewBox`), so
  the launcher icon is the only place the mark is written; `icons.rs`'s test holds the mask to the
  packaged paths and the asset name to the desktop entry's `Icon=` (`app.rs` likewise against `APP_ID`).
  gpui tints an SVG alpha mask by `text_color`: icons carry no colour, each gets one at the call site;
  `icons::lit_on_hover` follows its button's hover through a group. A two-tone mark = two overlaid
  `svg`s. No control uses a glyph. **A control's box and mark are one declaration:**
  `theme::Control::of` asserts in a `const` that the mark leaves `MARK_MARGIN_AT_LEAST` on every side;
  a control borrowing another's mark gets its own (`CLEAR`, `FILTER_CLEAR`, `CHECK_MARK`).
- **The launcher's icon wears the accent; the binary puts it there.** `launcher::AppIcon::of` is the
  whole decision: worn accent = `Appearance::DEFAULT`'s → `Packaged`, else `Recoloured`
  (`packaging/resonate.svg`'s gradient stops → the accent; root `id="resonate-in-the-accent"`).
  `AppIcon::drawn_here` reads that id; an icon under that name nobody here drew is left alone
  **unless it is a byte-for-byte copy of the packaged icon**, which takes the accent but is written
  with `data-written-over="the-packaged-icon"` so returning to Resonate's accent writes the packaged
  copy back (`a_copy_of_the_packaged_icon_takes_the_accent_and_is_put_back_after_it`). `run` shows the
  opening appearance, `RootView::dress` every later one, via the `Launcher` seam on `Stored` (no I/O in
  `resonate-ui`); the binary's `launcher::Icons` waits `SETTLES_AFTER` of quiet, writes a staged copy
  over `$XDG_DATA_HOME/icons/hicolor/scalable/apps/resonate.svg` or removes it, and only where that
  moved anything flushes the caches (theme folder mtime, `kbuildsycoca6`,
  `resonate_mpris::tell_the_icons_changed`, `gtk-update-icon-cache`). **The gradient is
  `userSpaceOnUse`**: under `objectBoundingBox` each stroke is a zero-width box that paints nothing.
- **The catalog is read while gpui starts, so the first frame holds it.** `run` starts `FirstRead`
  before `Application::new` (`Asked::at_first`, also what `LibraryModel::new` is built from) while gpui
  spends forty-odd ms on system fonts and Vulkan; the first read takes it synchronously where landed,
  awaits it where not, loads afresh where the model asks something else.
- **`resonate-codec` decodes and scales cover art; gpui gets pixels.** `Drawing::no_larger_than` and
  `Drawing::squared` answer a `Raster` (blue-first as gpui's atlas holds them); `models::picture_of`
  wraps it in a `RenderImage`, never re-encoded as PNG. A picture over `LARGEST_COVER_SIDE` (8 192) is
  refused before decoding; a shrink weighs each pixel by alpha in premultiplied linear light
  (`what_is_transparent_does_not_tint_what_is_shrunk_with_it`). Pictures are held in a `Recent` keyed by
  album, artist or file *and drawn side* (a fresh `Arc<RenderImage>` per frame = a new texture per
  frame). Read and decode run on `Drawer`'s `DRAWING_THREADS` (2) threads, not the background executor
  (glibc gives every allocating thread an arena, and a decoded 1280-pixel JPEG would stay in each of
  gpui's sixteen workers: ~110 MB). `Drawer` is a gpui `Global`: `draw` queues a closure and answers a
  `futures-channel` oneshot. A miss goes in the model's `decoding` set and the cell draws a placeholder
  disc until it lands; a picture a file lacks is cached as a miss, which `LibraryModel::reload` forgets
  (`Recent::forget_where`) so a fetched or rescanned picture draws
  (`a_cover_found_after_it_was_first_drawn_is_drawn_once_the_catalog_reloads`). **A file's picture is
  cached only once the engine settled it**: `PlayerModel::art` reads `Player::art_read`
  (`NotYet`/`Answered`/`Nothing`); `NotYet` goes in `unsettled` against the `media_revision` asked at;
  `Failed` is held in `unread` and asked again after `A_FAILED_PICTURE_IS_ASKED_AGAIN_AFTER`; a failed
  release cover likewise (`A_FAILED_COVER_IS_ASKED_AGAIN_AFTER`, `released_covers`).
  - **The tracks pane asks for row covers a screenful past what it draws**: after its rows, the
    processor hands `RootView::covers_ahead` the albums of the `COVERS_AHEAD` (48) rows past each end of
    the range, nearest first, to `LibraryModel::cover_ahead`, which leaves `DECODES_KEPT_FOR_WHAT_IS_SHOWN`
    (1) of the `DECODES_AT_ONCE` slots free for rows on screen and says when none is spare.
  - `LibraryModel::warm_the_covers` fills the album cache ahead of the grid once a run: `COVERS_WARMED`
    (256) albums, one decode at a time, bounded under `COVERS_HELD` (warming more evicts its own work).
    `DECODES_AT_ONCE` (4) bounds in-flight decodes in the album and per-file caches: a cell asking while
    full is refused *without* being marked decoding, so it asks again next frame.
  - **What a cache lets go of, gpui lets go of**: `Recent::insert` returns what it pushed out as a
    `Leaving`; `Forget` turns it into `App::drop_image`, removing its tiles from every window's sprite
    atlas, so GPU memory is bounded by the caches (the Listen sheet's picture:
    `Image::remove_asset`). 256 per-file pictures (`PlayerModel`, `PICTURES_HELD`) beside 512 album
    covers (`LibraryModel`, `COVERS_HELD`), a `BTreeMap` from use clock to key beside the entries so
    eviction is `pop_first`, never emptying itself. `LibraryModel::named` is emptied by `reload`, by no
    plain `read`.
- **A cover is drawn at twice the device pixels it is shown at, at that side alone.** gpui's atlas
  sampler is bilinear without mipmaps, reading four texels however far it reduces. `Drawn` names the
  cell (`InARow`, `NowPlaying`, `InAGrid`, `OnThePage`); `Drawn::side` asks the window's `Scale`
  (`Scale::texels_for`): twice the cell's device pixels below scale factor two, device pixels from two
  on. `RootView` hands the scale to both models whenever it moves; keyed by drawn side, a window dragged
  to another display redraws at the new size. Scaling is `CatmullRom` in linear light; a cover smaller
  than the cell is handed over as it came. `artwork::drawn_smaller` walks `image::imageops::resize`'s
  weights in memory order (the crate's vertical pass strides columns with a linear-light conversion per
  tap: two thirds of a cover's decode), bit-identical to it
  (`a_cover_is_drawn_exactly_as_the_image_crate_would_draw_it`): whole source rows added into a column of
  sums, each converted to linear light once into `Held`, the transfer function a 256-entry table.
- **The magnifier reads its picture when it opens, not kept beside every thumbnail.**
  `LibraryModel::whole_cover` and `PlayerModel::whole_art` read on a `Drawer` thread, no larger than
  `MAGNIFIED_AT_MOST` (4 096), keeping one `Magnifying` (the key being read, then the key with what it
  read, so a key answering nothing is not asked every frame); a read landing after another picture was
  asked for is dropped from the atlas. Nothing magnifies a portrait. **Its scrim occludes**, so the
  press closing it closes only it (a non-occluding `absolute` child is one more hitbox, not a wall);
  the menu's scrim occludes likewise.
- **A portrait is a picture cut square, held apart from covers.** `drawn_square` draws through
  `Drawing::squared` (the centred square of the shorter side, never grown). `LibraryModel::portrait`
  takes a `Portrayed` (`InARow`, `InAGrid`, `OnThePage`) and reads `portraits`, a `Recent` under
  `PORTRAITS_HELD` (256), drawn under the same `DECODES_AT_ONCE` bound only where
  `Artist::has_portrait`. `browser::portrait_frame` = the round frame at `theme::avatar()` in the
  artists list and `theme::scope_cover()` in the scoped heading; `kit::avatar` draws where there is
  none.
- **A cover is drawn wherever a row names a track, from one cell.** `RootView::cover` takes a
  `Pictured` (an album, or a track: `Option<AlbumId>` beside its file); album art preferred,
  `Player::art` the fallback. `RootView::drawn_cover` answers the art and a `Magnified` naming its
  source, so the transport cover opens whichever it drew; the overlay is sized from
  `Window::viewport_size` (`theme::magnified_cover`).
- **Text answering the pointer is lit when the element is built; gpui cannot light it later.** gpui
  0.2.2 resolves a `hover` style only in *paint*, while text is shaped in `request_layout` with the
  moment's colour baked into its runs: a `hover` changing a background works; one changing `text_color`
  or adding an underline never reaches a glyph. `views/pointed.rs`: an element following the pointer
  carries an `on_hover` writing its `ElementId` into the `POINTED_AT` set (thread-local) and asking for
  a redraw; the next build reads `is_pointed_at` and shapes the text lit.
  `LitUnderThePointer::lit_under_the_pointer` is the gesture; `follows_the_pointer` its half for an
  element lighting something inside it. A set, not one slot (the pointer over the artist under a cover
  is over both). `set_pane` and `show_everything` empty it; the pointer leaving the window empties it
  via `pointer_watch`.
- **A hint names its key one way, and the key is the one bound.** `keys.rs` is `key!` (`key!(play_pause)`
  = `space`); `app::bindings` binds through the same `key!`. `keyed!` writes the wording: what the key
  does, an em dash, the ways (last two joined by *or*): `Louder — the wheel or ctrl-up`; a hint saying
  more joins such sentences with full stops, a described state first. Both are `concat!`, so hints stay
  `&'static str` (`every_key_a_hint_names_is_a_key_something_is_bound_to`).
- **A control names itself through one seam, and the seam lets go.** `hint::Names` is the only place a
  tooltip is attached (a test walks the crate's sources): gpui's plain `tooltip`, never
  `hoverable_tooltip` (neither press nor scroll takes that one down). `hint::asking` is one atomic
  written once a frame by `RootView::render` (`hints_are_wanted`); where it says no, no builder is
  attached, which gpui reads as an instruction to drop a live tooltip. No while a slider is grabbed, an
  equaliser band held, the picker (`adding`), a magnified cover, a record or a deletion prompt is up, a
  row is dragged, the pointer is outside the window, and for the one frame in which what lies under an
  unmoved pointer did move: `RootView::laid_out` stamps pane, settings category, queue revision, the
  listings, playlist count and landing scroll offset, and `UnderThePointer::moved_since` weighs it
  against the frame before (gpui hit-tests only on a mouse event). gpui's `MouseExited` arm does not
  update `Window::mouse_position`, so `RootView::pointer_watch` = a zero-size canvas registering
  `MouseExitEvent`, `MouseMoveEvent` and `MouseDownEvent` window listeners from its *paint* (the only
  phase `Window::on_mouse_event` may be called in); its flag also closes the lyrics pane's hover.
  Every control carries a tooltip; a toggle names its state and the key changing it.
- **A playlist row no scan has seen draws as a queued one does.** `views/listing.rs` owns the `Row`
  the queue and playlists panes build: the scanned `Track` where the library has one, `Player::media`
  where not, the file stem only where the file cannot be read. The play count rides on the same `Row`
  (`listing::heard`: one `theme::row_plays()`-wide cell for the tracks pane, both list panes and the
  playlists index); `how_often_and_how_lately` folds the date into it (*12 plays · 3d*, header HEARD,
  `COUNTS_AS_HEARD`); `format::age` fits it from the `SPANS` table `format::since` reads
  (`every_span_is_named_both_ways_and_no_two_read_alike`).
- **A catalog listing marks the playing track by the row it resolves to, never the queue's id.** The
  tracks pane, Favourites and an opened suggestion compare against `RootView::playing_now`'s `track`,
  the library row `LibraryModel::track_of` weighs out of the queue row's location and span (a resumed
  queue is minted afresh by `Queue::restore`). What `track_of` read is held in `named`, the albums the
  bar links to in `read_albums`. `LibraryModel::reload` forgets the albums but keeps `named`, serving
  what it held while one background batch (`queued_rows`, `name_what_is_held_again`; a newer reload
  replaces the batch) reads every held row again and swaps in the ones that changed, so a redraw
  after a reload reads nothing on the window's thread; only a row never read before is read where
  it is drawn (`a_queued_row_is_read_again_once_the_catalog_changes_under_it`). Statistics' *play from
  here* reads its tracks with one `held_tracks` batch (`Library::tracks_with_ids`).
- **The row playing is the row a list is on, never a track id** (a file can be in a queue or playlist
  twice; an unscanned row has no `TrackId`). The queue draws `PlayerState::queue_position`; a playlist
  `PlayerState::loaded_position`, marked only while `Library::playing_playlist` names it, i.e. while
  the queue holds the rows it was loaded with.
- **An edit never cancels the one before it.** `LibraryModel::edited_then` holds one task in `_edit`;
  each new edit takes the previous out of the slot and awaits it first
  (`two_edits_asked_for_at_once_both_land`); `track_passed` and the two `listened` writes on `_settled`
  chain alike.
- **A read of the library says how much it wants.** `LibraryModel::read` takes a `Wanted`.
  `Everything`: browse panes, what stands whatever is typed (statistics, most listened, days,
  suggestions, roots: `Standing`), shelves (playlists, wants, missing: `Shelves`). `TheSearch`: browse
  panes and shelves, leaving the standing and the wants (`reads_the_wants`). `ThePage`: the four lists
  `reach` bounds (albums, artists, tracks, scoped tracks: `Paged`) alone, for `reach_further`.
  `ThePlaylists`: shelves alone, wants included. Every gesture behind `LibraryModel::edit` writes only
  playlist tables, so an edit, a listing-order change and opening a playlist take `ThePlaylists`; a
  scan, a counted play, a window of time and forgetting a root take the whole. `Loaded` carries each
  part as an `Option`; `renewed` swaps a list's `Arc` only where what came back differs
  (`a_list_read_again_the_same_is_left_where_it_stood`). **A narrower read never cancels a wider one**:
  a read replaces `_load`, so `in_flight` is remembered and a read asked while one runs reads what both
  would have (`Wanted::with`: `Everything` over anything, `TheSearch`+`ThePage` → `TheSearch`,
  `ThePlaylists` with those → `Everything`;
  `a_read_asked_for_while_another_runs_covers_what_both_would_have_read`).
- **A read the typing asked for waits for the typing to stop; every other runs at once.**
  `LibraryModel::set_query` is the one caller giving `read_after` a delay, `SEARCH_SETTLE` (150 ms).
  The query and `Search::read` move at once (the *Reads* row is live as typed); only the SQLite work
  settles, in the `_load` task, so a keystroke inside it drops the one before.
- **Every notice is a toast: a pill that rises over the playback bar, lingers and goes.** `toast.rs` is
  the whole of it. `Toaster` is a gpui global, so engine failures, library passes, the equaliser and
  every gesture tell it alike (`toast::tell(notice, cx)`). One pill at a time, `type_ahead_lift` above
  the bar; rises `RISE` (10 px), fades in over `ARRIVAL` (220 ms), lingers `LINGER` (6 s;
  `LINGER_IN_A_BURST` 2 s where more wait; at most `WAITING_AT_MOST` (4) waiting, never the same words
  twice (a `Trouble` told again while the same one shows or waits is counted on it, *… (×3)*, and a
  repeat of any other tone is dropped); a full line gives up its oldest non-`Trouble` for a newer notice, turns a plain one away where
  only troubles wait, and keeps every `Trouble`, so troubles alone may pass four), fades out over `WITHDRAWAL`; a click or escape takes it early. A `Notice` is `Trouble`,
  `Done` or `Noted`; the tone is the pill's icon (alert in the failure colour, tick in green, info mark
  in the accent). `EqualiserModel` sets its notice where it holds no `cx`: an outbox `RootView` drains
  (`take_notice`). One line stays in place, belonging to its control: the Library card's refusal of a
  typed layout field. **A finished pass is a toast too, never a line left standing**: a scan, organise,
  retag, vault import and inbox poll draw counts in their group only while running (`stats`,
  `poll_stats` answer `None` once over); the model tells what came of it as it joins (`scanned`,
  `looked_up`, `filed`, `retagged`, `vaulted`, `polled`). A scan or poll the window started on its own
  is told only where it brought something. A *preview* keeps its summary in the group.
- **Toasts speak plainly; the error goes to the log.** No error `Display` reaches the pill. The engine
  names failures `resonate_engine::Cause` (`Unreadable`, `Unsupported`, `Damaged`, `NoDevice`,
  `DeviceGone`, `SoundServer`, `DeviceRefused`, `CannotSeek`, `QueueMoved`, `NothingPlaying`,
  `PlayerStopped`), read exhaustively off codec/PipeWire errors in `Error::cause`; `Error::location`
  names the failed file. `toast::would_not_play`/`would_not_do` turn a cause into words; library and
  equaliser failures use `toast::could_not("start the scan", &error)`. A notice is a label: no full
  stop. A press that reads the catalog before it plays or queues (heading Play and Shuffle, a scoped
  listing, a suggestion, a dragged album, a downloaded song) toasts the failed read as well as logging
  it; a settings write whose writer died is told like one that failed; so is a cover, portrait or pressings read, a play
  counted and a listen time kept (`picture_or_trouble`, `tell_of_listen_time`).
- **No device name in the playback bar** (the sink description pushed the album title off). The
  inspector's OUTPUT stage names the device; the signal path's hint says *Playing through …*.
- **Thin root over four cached regions: a moving clock redraws only the playback bar.** gpui re-runs the
  root view each frame, reusing layout/paint only for a child drawn as a *cached* `AnyView` nothing
  notified. `views/part.rs`: a `Part` per region (header, sidebar, pane, bar) renders
  `RootView::region` via a weak handle (panes stay `RootView` methods, listeners bind to the root);
  `RootView::render` = shell (actions, overlays, menus, toasts). Each `Part` observes the root; a
  hover/scroll/frame asked inside a region notifies it alone. A cached view is laid out from a style,
  not content, so `Region::laid_out` declares sizes. **The pane is held in flex columns, its root the
  one item** (`Region::holding`): width is the cross axis, stretched and never measured; a pane root
  fills the height by `size_full` or is `kit::pane()` (column, `flex_1`, `min_w(0)`, `min_h(0)`), since
  height is the main axis, whose automatic minimum is the content's (a `flex_1` root without
  `min_h(0)` left the settings body and lyric sheet nothing to scroll). A one-cell grid
  (`minmax(0, 1fr)`) held it once and broke every cut name in plain divs (below).
  - `PlayerModel::refresh` reports a poll as `Moved`: `Clock` if only position and sink latency changed
    (`held_still`, `Grain::shows`), else `More`. `More` notifies the root; `Clock` →
    `Parts::the_clock_moved` (bar, plus pane where `Pane::follows_the_clock`: inspector latency,
    analysis playhead).
  - **`Grain`** (`EveryPoll`|`Stepped`) = how often the clock moves, told by the render to the model.
    Every poll: visualiser, held seek thumb. Else a quarter device pixel of the seek rail
    (`STEPS_PER_PIXEL`), never coarser than `CLOCK_STEP` (1 s), so `Listening`/`Keeping` never see a
    step near the `A_SEEK` they read as a seek. A synced lyric set asks a frame at display rate while
    it moves. `Grain::shows` passes anything but position/latency at once; an unpainted rail is every
    poll.
- **Playback bar = three columns, transport in the middle.** Now-playing panel and status cluster:
  `flex_1`, zero basis, ≥ `SIDE_AT_LEAST` (180 px), equal halves of what the centre leaves, so the
  buttons sit on the window's centre line. Centre: basis `theme::transport_centre()` (460), shrinks to
  `CENTRE_AT_LEAST` (260), gives way first. A press on the total clock toggles time left
  (`RootView::showing_time_left`, not kept; `format::left` subtracts whole seconds so elapsed plus left is the length shown, and a row opening (`Buffering`) draws and acts as *Pause*); the rail fills its row (`Handle::fills_its_row`). Both
  sides `overflow_hidden`. **The seek rail names the time under the pointer** in a bubble
  (`slider::Pointed`, `RootView::seek_pointed`; drawn, not a `hint::Names` tooltip; nothing where
  length is unknown).
  - **What does not fit is left out whole, never clipped.** `views/gives_way.rs` = arithmetic over theme
    measures, no gpui. Status cluster measured into `RootView::status_room` a frame behind;
    `status_kept` drops volume reading, sleep control, repeat, shuffle, then volume rail, until the
    rest fits; never the queue button or the speaker. `signal_kept` likewise against `playing_room`
    (`kit::width_of`): mode's name first, then depth and rate, then the mode's dot; codec badge stays.
  - **Now-playing panel**: cover (`theme::now_playing_cover()`), title, artist, album, signal path:
    decoding-format badge with depth and rate, then the output's mode dot and name in the mode's colour
    (*bit-perfect*, *converted*), nothing after (the inspector spells out what a converted stream
    became). A press opens the inspector. **The path is as wide as what it says**: `self_start`
    under `max_w_full`, else the column stretched it and its hover wash ran to the panel's edge
    (`the_signal_path_is_as_wide_as_what_it_says_rather_than_the_panel`).
  - **A skip never blanks the panel.** While the next row opens the engine publishes no current track
    (`Buffering`, or `Paused`: `between_songs`): `RootView::playing` answers the song last resolved,
    `name_the_window` keeps its title, `held_through_a_change` draws the last cover
    (`RootView::shown_cover`) up to `COVER_HELD_FOR`, `held_signal` holds the signal path. **The last
    cover is held only while the next song's picture is still unknown**: once the caches hold it as
    absent (`RootView::is_pictureless`: `cover_is_absent`, `art_is_absent`, both `Recent::peek`) the
    held cover is let go at once and the disc crossfades in, not after `COVER_HELD_FOR`.
  - **Title, artist, album are three elements**, each opening its page via `RootView::opens`:
    title/album scope the tracks pane to `Selection::Album`, artist to `Selection::Artist`. A scope
    whose album/artist the catalog no longer holds (`Browsed::gone`) becomes `Selection::Everything`
    when the reading lands. Pressable = *known*: album id rides on the resolved `Cover`, artist id on
    `Track::artist_id`; an unscanned row draws all three plain. `RootView::by_line` = artist,
    separator, album (the inspector's heading is the same line); `keeps_its_width` holds the artist at
    `flex_none` under `max_w_full` so the album gives way first.
  - **What gives way ends in an ellipsis, not a square cut, each given a width it can be cut at.**
    gpui's text element truncates only inside its measure and keeps a no-wrap text's first measure: a
    `truncate`d name in a content-sized flex item is laid out whole and sliced by the parent's clip; a
    `line_clamp` there is measured at min-content width and draws nothing. Title and album go through
    `kit::cut_to_fit`: title at `RootView::playing_room` (a frame behind) less the star; album at the
    room `by_line` is handed less artist and separator (`kit::width_of` sums font advances; a glyphless
    character counts one em); no room left = artist alone. The inspector uses `inspected_room`.
  - **Every bar control names itself via `views/hint.rs`**: a step names its key, a toggle its state
    and what the key does next. A rail is filled in `TEXT`, accent under the pointer or held. The
    volume reading is always drawn, `theme::volume_reading()` wide (`opacity(0)` left a hole);
    `kit::readout` draws it in `theme::ui` (body face with `tnum`).
- **The play mark carries its optical nudge in the art, not a margin.** `play.svg` sits half a unit
  right of the 24-box centre because a triangle centred by bounding box reads left; a `margin-left`
  on the play state alone moved the mark sideways on every toggle.
- **Lyrics: the look starts with the track, and the pane follows the playing track alone.**
  `look_for_lyrics` runs on `RootView`'s observer of `PlayerModel`, so a track change searches whatever
  pane is in front. `RootView::wanted_lyrics` builds a `Wanted` from the queue row and `StreamDigest`
  tags, filling title, artist, album, length off the scanned row until the digest lands. A redraw
  compares `lyrics::Asked` (`Copy`); `LyricsModel::asks_again` gates building `Wanted`. `follow` resets
  the pane only where the *track* moved; `worth_looking_again` asks on a track change, else only where
  `Wanted` differs from the last searched. The look runs on the background executor; lines become
  `SharedString`s once, when it lands. `attribution` ends the heading: a `SYNCED`/`UNSYNCED`/
  `WORD-SYNCED` badge, the provider's name as a `kit::figure`, and one figure for the most particular
  credit (`[by:]`, else `[au:]`, else the editor) whose hint carries the rest.
- **The pane always draws and scrolls the whole sheet; a reading is only how far the light reaches.**
  `Falloff::Around` looks only forward (`Reading::InPlay`); `Falloff::Across` steps down gently both ways
  (tail 0.16) for `Reading::Whole`. Drawing every line either way is what lets lines slide, not swap.
  The choice is a heading chip living for the run; the pointer over the column (`near_the_words`,
  via the `pointer_watch` idiom; an `on_hover` opened it from the empty gutters) or a scroll of your own
  opens it out: `LyricsModel::shows_every_line` weighs `following`, which runs out `HANDS_OFF` (6 s)
  after a hand scroll, and `spread` closes the sheet again over one `TURN`. An unsynced set has no lit
  line (`ADRIFT`, no chip). **The sung line brightens; it does not grow**: gpui floors a glyph's vertical
  origin to a whole pixel and cosmic-text hints every size, so a size in motion shimmers. A second voice
  takes the trailing edge (and the accent when lit); the falloff counts written lines, not rows.
- **Motion: three `Turn`s on one 420 ms `TURN`, and a `Glide` on a cubic.** `Reads` (`At` a line,
  `Spent` in a wait, `Evenly` for unsynced) is what the light turns on; `standing` blends over it,
  `lead` over the lines in play, and `spread` over the `Falloff` (via `Turn::onto`). **Nothing lit is
  not an empty pane**: with nothing in play and no wait begun the turn stays `At` the line just read
  (colour falls to `muted`); a wait goes to `Falloff::spent`, leaving the line waited on and the one
  after readable under the dots
  (`a_gap_shorter_than_a_breath_holds_the_line_just_sung_rather_than_putting_the_sheet_out`,
  `a_pause_leaves_the_line_it_waits_on_and_the_one_after_readable_under_the_dots`). The `Glide`
  carries the scroll offset to the read line's `landing` over `GLIDE` (600 ms), `1 − (1 − s)³(1 + 3s)`:
  at rest at both ends, never passing its landing; a cubic, not a spring, because on whole pixels the
  last pixel lands ~35 ms after the one before
  (`a_glide_lands_on_a_whole_pixel_with_its_last_step_close_behind_the_one_before`). `centre_of` reads
  `ScrollHandle::bounds_for_item` (layout with no scroll offset), so the landing *is* the offset; a
  landing within `SETTLED` (half a pixel) is no move, any other `redirected`s the glide as a new `Leg`
  carrying its pace (`LAUNCHED_AT_MOST`), the old one kept as `Glide::before`
  (`a_new_landing_in_flight_carries_the_glide_on_without_a_jump_or_a_stop`). A gap is read at the line
  it waits for (`read_at`: `Waiting::next`, then the ended row, then `line_at`). A line `n` written
  lines past the read line is where the sheet was `LAG_PER_LINE × n` ago (`Glide::lagging`, capped at
  `LAGS_AT_MOST`; each row is an outer box `bounds_for_item` measures and an inner `relative()` box
  inset by `LyricsModel::inset`). `place` starts `arrived`: `arrival` (opacity over `ARRIVES_IN`) and
  `rise` (inset from `RISES_FROM`, `RISE_PER_LINE` later per line); `breath` is a sine over `BREATH`.
  `looks_quietly` keeps a look under `LOOKS_QUIETLY_FOR` (450 ms) from flashing *Looking for lyrics…*.
  A placed unsynced sheet's `landing` answers nothing, leaving the reader's scroll
  (`a_plain_sheet_is_placed_at_its_top_once_and_then_left_where_it_was_read_to`). Once
  `Lyrics::has_ended` the read line is `end_of_the_sheet`, an *END OF LYRICS* mark.
- **The sheet grows with its pane and moves on whole device pixels.** `Growth::of` takes the tighter of
  the pane's width/height ratios against `PANE_AT_RESTING_SIZE` (784 × 600), held in 1..`GROWS_AT_MOST`
  (2.5); every sheet length is `Measures::at(scale, growth)`, growth being layout's, not motion's; a
  resize moves `Measures`, which `place` weighs, so every line is laid out again before any is held open
  at an old height (`the_sheet_grows_with_its_pane_by_the_tighter_of_its_two_sides`). gpui draws text on
  whole pixels, so `Scale::snapped` rounds `landing`, `glide`'s offset and each line's `inset`
  (`snapped(line) − snapped(sheet)`, once: rounded apart they ticked back and forth,
  `every_line_is_drawn_on_whole_pixels_and_never_steps_back_through_a_glide`), and `Measures` holds every
  vertical length snapped at every growth, since taffy rounds a size by position
  (`the_measures_of_a_line_are_whole_device_pixels_at_any_scale`).
- **A line far from the pane is held open at its height, not laid out.** `LyricsModel::resting_height`
  answers the height `bounds_for_item` last gave a line more than `DRAWN_WITHIN_PANES` away, only while
  `LyricsModel::steady` (pane size and `Measures` equal two frames running: gating on the frame's own
  size let a zero-wide first layout hold thousand-pixel heights;
  `a_line_is_held_open_only_once_two_layouts_running_were_measured_alike`). `lyric` draws it as an empty
  box, so the child indices `centre_of` reads stay put.
- **A pane is drawn nowhere until placed; a row is as wide as the pane says.** Spacers are measured off
  `ScrollHandle::bounds`, a frame behind; `place` reveals nothing (body `opacity(0)`) until `steady`,
  snaps instead of gliding, and reports itself moving so frames keep coming while paused; `rewind`
  resets on a track change. Each row takes an *absolute* width, `LyricsModel::column_width` (pane width
  less `Measures::gutter` each side, capped at `Measures::column`), not `w_full` under a `max_w`: taffy
  fixes a flex item's height from a measure taken before a percentage width resolved, so wrapped lines
  were one row tall (the same goes for the settings body). A synced sheet's head is the container's top
  *padding*, its tail a spacer *child* (`content_size` is the children's extent alone). The pane asks
  its own frames (a zero-size `canvas` calling `Window::request_animation_frame` while any clock is in
  flight); the 16 ms poll (`POLL_INTERVAL`) notifies only when state *changed* and **rests while nothing
  moves**: `PlayerModel::refresh` answers `Poll::Quiet` when not playing, no sleep timer, no seek in
  flight and nothing changed, and after `QUIET_POLLS_BEFORE_RESTING` (30) the loop waits
  `RESTING_POLL_INTERVAL` (64 ms).
- **A sheet that runs early or late is moved by the listener, for that track.** *Later*/*Earlier*
  (`LyricsAhead::STEP_MS` 100, within `MOST_MS` 30 s; `resonate-library`'s `sung.rs`) and the offset as
  a chip putting it back. `look_for_lyrics` reads `Library::lyrics_ahead` into
  `LyricsModel::hold_ahead`; `nudge_the_lyrics` writes `Library::hold_lyrics_ahead`. The pane reads every
  line at `ahead.read(clock)`; a line press seeks to `ahead.back(moment)`
  (`RootView::seek_to_moment`, clamped to the duration). A wheel sets `led_by_hand` (no following for
  `HANDS_OFF`, heading shows *Follow*); a press or track change resumes.
- **The pane reads its own clock, not the engine's position.** `PlayerState`'s position climbs a block at
  a time (~90 ms FLAC). `LyricsModel::keep_time` runs a wall-clock `Clock` from the last sample, steered
  to each new one by `1 − e^(−t / STEERED_OVER)` (a share of time, not frames), never back; a seek, new
  track, pause or drift past `DRIFTS_AT_MOST` takes the published position as is.
- **A gap breathes, mid-song and before the first line.** `Lyrics::waiting_at` answers only where no
  line is in play (`lyrics.md`). Three dots stand where that line will be, filling in turn, swelling on
  `LyricsModel::breath` by `DOT_SWELLS_BY`; `breather` is a canvas of the *fully swollen* size, each dot
  a quad round a fixed centre. **The dots never change the layout: their room is the sheet's.**
  `Lyrics::breathes_before` gives from timing alone the lines a wait counts down to (the first written
  line; any whose previous line goes out `A_BREATH_AT_LEAST`, 3 s in `resonate-lyrics`, ahead); each has
  `Measures::breath` of room above its text for the sheet's life, and the dots only fade
  (`Breathing`, `FadingBreath`). Progress *through* a line only where the set times words.
- **A name the window can open is an `opens`; its id decides.** `RootView::opens(…, Option<Selection>)`
  wraps `pressed_to`: `Some` = link (accent+underline under pointer, cursor, hint, listener selecting
  and showing `Pane::Tracks`), `None` = the same truncating element without them. Every pressed name
  uses it (bar title/artist/album; inspector and lyrics headings; owner under a scoped album heading;
  artist under a grid cover; artist cell of every row). The listener stops the press. The link is
  `flex_shrink` under `ends_in_an_ellipsis` in a cell, as wide as the name. `listing::Row` has
  `artist_id` beside `artist` (`listing::scanned` fills it; `None` from `read`/`unread`/`unheld_row`).
- **A name past its room ends in an ellipsis; `kit::EndsInAnEllipsis` is the one place that knows how.**
  Bare `truncate` cuts silently: under a content-sized flex item no ellipsis came (taffy measures under
  `MaxContent` and gpui's text cache answers that for every later measure of `nowrap` text).
  `ends_in_an_ellipsis` = `whitespace_normal` + `line_clamp(1)` after `truncate()`: a single-line *wrap*
  escapes the cache, the clamp draws the ellipsis. **No `truncate` stands alone** except two in
  `views/search.rs` (a quoted query inside `kit::title`, a section's faint hint line). **A content-sized
  box does not wrap**: `kit::KeepsItsWidth::keeps_its_width` (`flex_none`, `max_w_full`) sets
  `whitespace_nowrap` back over the clamp (taffy measures such a box at min-content width, so wrapped
  clamped text drew nothing); those boxes are cut by `kit::cut_to_fit`; a `flex_shrink` cell keeps the
  clamp.
- **gpui truncates the runs it is handed, once for every measure.** gpui 0.2.2's `TextLayout::layout`
  moves one `Vec<TextRun>` into its measure closure and `truncate_line` cuts it in place on every call;
  if taffy measures narrower first and wider afterwards, the wider layout keeps the narrow runs (a 0 px
  measure cuts them to the ellipsis's three bytes: underline, highlight colour and weight stop there).
  A grid with `minmax(0, 1fr)` tracks causes it (taffy measures the item at the column's 0 base size;
  `uniform_list` rows escape it). Any new layout around a plain-div listing is checked by an
  `eprintln!` in `TextLayout::layout`'s measure closure of a `[patch.crates-io]` gpui copy and a driven
  test rendering a long cut name.

## Driven by tests

- **A pane is pressed, dragged and scrolled by a test under gpui's test platform.** `resonate-ui` has
  gpui `test-support` as a dev-dependency; `driven.rs` is the harness. `Driven::open` sets the
  `ResonateApp` global as `run` does (`Player` over an `Unplugged` backend; in-memory `Library`;
  `Ephemeral` settings; `Places` in a temporary folder) and opens a real `RootView` in an activated
  `VisualTestContext`. It presses, right-presses, drags, scrolls and moves the pointer at bounds gpui
  drew, read via `debug_selector`: `kit::Found::found_as` names every button, chip, segment, switch,
  choice and mark by `ElementId`; rows, tabs, menu entries, surfaces name themselves (`tab-visualiser`,
  `track-<id>`, `queued-<id>`, `menu-entry-<n>`, `picker-<id>`, `equaliser-curve`). `Driven::settle`
  runs the executor until it parks and advances the test clock a frame; `Driven::until` waits on the
  real engine and scan threads; `Driven::play` opens rows via `Command::Resume`, the first paused.
  Microphones are planted via `ListenModel::hearing_of`; `Driven::recognising` hands the window a
  `Fingerprinters` whose fake printer hears the playing file as another song;
  `Driven::reaching` a `Reaching` naming the reference and providers. A control below the fold is out of reach of a press (`Driven::scroll` the
  settings body first); a switch is pressed by its `-track` mark. Every Output and Processing choice and
  switch is pressed and read back off `Player::output_settings`; `Driven::hearing` hands the
  `Unplugged` backend sinks to list, so a device row is pressed too. gpui can't build an
  `ExternalPaths` in a test: `dragged_over` and `dropped` are called on the `RootView`; the platform's
  own drag events are driven by the `drive-a-drag` skill on a hosted KWin, outside the test suite. `views/root.rs`
  drives keys. The equaliser's note is always drawn and `kit::section_header` holds a row control's
  height, so pressing the curve with the equaliser off moves nothing under the press.

## Panes

- **A favourite is a star; the heart is taken; missing is neither.** `Icon::Want`/`Wanted` (heart) are
  the *gesture* of wanting a row only; the Missing pane and an artist's *not held* button use
  `Icon::Missing`. `Favourite`/`Favourited` = star. The mark hides on hover like every row control
  **unless the row is a favourite**; everything that fades must be inside what fades (a conditional
  control's ground, border and padding belong inside the condition). **A favourite is filled in the
  accent** (`kit::star`).
- **The star says what was favoured from the press on, not the next read.** `LibraryModel::favour`
  records the write in `favoured` until the read after its write lands (`favours_written`), puts the
  answer into the `named` row where the track is cached, and notifies before the write runs. `favours`
  is what every star, cell and menu asks (`favoured_album`/`favoured_artist`/`favours_track`); bar,
  queue and opened playlist ask it too, their row from `named` keyed by the *queue's* id (resumed
  queue, unscanned playlist row and doubled row carry ids minted apart from the catalog's).
  `listing::scanned` takes the answer, not `Track::favourite`. A failing write removes its entry
  (`Edited::Failed`, `unfavour_what_did_not_save` restores the replaced `named` row).
- **Three panes, each the same six edits.** `Pane::Favourites`, `Pane::Suggestions` under
  `Section::Collection`; `Pane::Statistics` under `Section::Library`. Each: a variant, a place in
  `Pane::BROWSE`, arms in `label`/`about`/`section`/`icon`, an arm in `RootView::content`, a sidebar
  count, a `mod` line. Favourites stacks artists shelf, albums shelf, track rows; its count is all
  three. Its track rows have their own order, `Sorting::favourites` (*Favourited* newest first until a
  header is pressed) and `sorting::favourites_sorted`, a `Sorted` with `#` as `Sortable::Marked`.
- **The statistics chart is `div`s, not a canvas** (a few dozen discrete bars; a `div` carries a
  `hint::Names` hover free). A day with no plays still draws its baseline; a run over `BARS_AT_MOST`
  (120) folds whole days into one bar. Axis: *today*, *yesterday*, else the date via
  `resonate-core::CivilDate` of the day `Calendar::local` puts each bar's midnight on (the calendar
  history buckets plays by); `Chart::in_calendar` takes one so tests hold the axis to UTC and a zone
  east of it (`a_day_is_dated_in_the_listeners_calendar_rather_than_greenwichs`).
- **Suggestions: shelves by kind; a card opens onto what it would hold.** `SuggestionKind::ALL` groups
  cards under *From your listening*, *Eras*, *Genres*, *Artists*, *Sound*, each an eyebrow over a
  wrapping row. Pressing art or name = `LibraryModel::open_suggestion`: `SavedQuery` kept in a
  `Previewed`, first `PREVIEWED_AT_MOST` (500) rows read on the background executor; `preview_further`
  grows the window whenever the list is drawn within `LOOK_AHEAD` of full. The opened pane draws a way
  back, art at the text column's height (`Drawn::OnThePage`), kind, name, reason, count and length, the
  search as *Reads* chips, Play, Shuffle, save and search marks, then an ordinary unsorted track
  listing, each row playing the list from itself. A suggestion the catalog stops offering returns the
  pane to the shelves. The search mark = `search_instead` + `choose_pane(Pane::Tracks)`; *Shuffle*
  picks its start row off the clock's nanoseconds (`somewhere_in`; no random crate in the tree). A
  card reads its rows off the frame (`RootView::with_the_rows_of`, sibling of
  `with_everything_listed`); *Save* = `Library::save_query` through `LibraryModel::edit`, a taken name a
  `Notice::Trouble`; a card whose search a playlist already fills greys a check.
- **A suggestion's art is drawn, never stored; a playlist's is the same drawing.** `views/mosaic.rs` is
  the one builder: `Mosaic` = covers drawn, two accents, a mark, a name; `Framed` = `Alone` (bordered,
  rounded all round) or `OnACard` (foot square). `suggestion_art` draws covers in
  `Suggestion::pictured_by` (one whole, or two to four as a 2×2 mosaic) over a 135° gradient between two
  accents of the worn palette via `theme::hue` (the reason's colour where it has one; a genre's or
  artist's from `resonate_core::Accent::named_pair`, an FNV hash of the name the phones share). **The gradient is painted only where no cover is** (gpui
  clips a child to its parent's rectangle, not its rounding). Side = whole pixel, near tile the floor of
  half, far tile the rest (`tile_span`); tile and cover round on the one corner they stand in
  (`Corner::of_tile`). **A frame standing alone draws its hairline over the art, never as its own
  border** (`border_1` shrank the tiles' box and the clip squared three corners): `hairline` =
  absolute, rounded, bordered child laid last.
- **The playlists index is a grid of covers or a list; the heading chooses.** `PlaylistsDrawn` = `Grid`
  (default) or `List`, a `kit::segmented` beside the sort icon, kept for the run. Grid has the albums
  pane's shape (`grid_width`, `grid_columns`, `grid_row`): mosaic at `theme::grid_cover()`, name, count
  and when last played; the pin sits in the card's corner (hidden until hovered unless pinned), a round
  accent Play rises under the pointer. List rows carry a context menu beside their controls; gpui 0.2.2
  starts a click only on a left press, so a right press on a control opens the row's menu and fires
  none. A pinned row wears a `kit::badge` beside `SEARCH` and `KEPT`. `Listed::Playlists` makes either
  shape a reach-key listing.
- **What pictures a playlist is read with the listing, never on the frame**:
  `Library::playlist_pictures` for every listed playlist and the opened one, `PICTURED_AT_MOST` (4)
  covers each. **Pinned playlists stand in the sidebar under Playlists**: `Library::pinned_playlists`,
  at most `PINNED_IN_THE_SIDEBAR` (5), most lately pinned first, unnarrowed by search
  (`RootView::pinned_rows`); the index still lists every pinned playlist first.
- **An empty opened playlist says why and offers the way out** (*Add songs* for a list, *Edit search*
  for a saved query matching nothing; a narrowed playlist says the search matched nothing in it).
- **The sleep control draws no clock of its own.** It opens a `Menu` rather than cycling: seven
  choices (`SLEEP_MINUTES`' five lengths, end of track, end of queue), eight while a timer runs. Accent
  wash and countdown sit in one `when_some` on `PlayerState::sleeping`; a part second rounds up.
  Deliberately not a setting: a sleep timer is a nightly decision.
- **The queue is reached from the playback bar, not the sidebar.** `Pane::Queue` only by the bar's
  queue button or `ctrl-u`, a `toggle` accent when open, no count; `RootView::behind_queue` remembers
  the pane the button covered.
- **The queue follows the row that started playing, only while it is the pane in front.**
  `RootView::following` is a `Following` (the `PlayingRow` last shown: `PlayerState::queue_position`
  row, the track's id, the queue's `queue_stamp`); `Following::follows` is the whole decision: the row,
  or nothing where the pane is not `Pane::Queue`, nothing plays, or the row has not *moved on*. A row
  moved only because rows above were dropped, dragged or put back changes the stamp and keeps the
  track: recorded, not scrolled to. It lifts the *NOW PLAYING* heading to the pane's top
  (`QueueParts::opens_at`, `lift_the_playing_row` scrolls strictly: gpui's `scroll_to_item` does
  nothing for a line in view); `QueueParts::room_below` adds blank lines where a heading near the end
  could not reach the top, and the list is not built until `queue_height` is measured. Opening the
  queue forgets the row last shown; a row that moved while another pane was in front is not recorded.
  `Following` is windowless arithmetic tested in `views/root.rs`.
- **The *Reads* row reads back the search box; it claims nothing about a listing.** `listing::reads` =
  one chip per clause off `Search::reads` in every pane's heading (counts only beside albums and
  artists). `Matching::grouped` carries the whole grammar into albums, artists and tracks,
  `db::cuts_matching` into an opened playlist's rows, while the playlists index narrows on `words_of`
  alone. A lone word draws no chip. A pane with nothing left says "No albums match." where a search is
  in force, "No albums yet." where none is, and "The library couldn't be read." (with a toast)
  where the last read failed (`LibraryModel::unreadable`).
- **The window keeps the queue for the next run; the switch stopping it discards what was kept.**
  `RootView` holds a `Keeping` (field `resuming`) off the `PlayerModel` observer, keeping what
  `resonate play` keeps (`audio.md`). `Stored::resume` is a `config.toml` key; the *Resuming* card is a
  Library group (it writes to the catalog). Off = `Library::forget_resumption` at once. Each write
  awaits the one before it (`keep_after_what_was_kept`).
- **Tab, restore size and Settings category each have a memory switch** (`remember-tab`,
  `remember-window-size`, `remember-settings-category`, default on, via `Stored` into `ResonateApp`).
  The tab is the pane `in_front` names; a tab hidden by `Tabs` is ignored on the next open. The size is
  written after `WINDOW_SIZE_SETTLE` (400 ms) so a drag edits the config once. Switching off removes the
  saved value.
- **The window is handed its lookups and owns none.** `Lookups` is what `run` takes: `lyricists`,
  `fingerprinters`, `reference` (`Option<Arc<dyn Reference>>`), `for_the_pass` (`Consulted`),
  `scrobblers`, `signs_in`, `corrections`, the `online` setting, `bindings`, `sourcing`, `listens`;
  `ResonateApp` holds them as globals, so `LibraryModel::new` knows whether it `can_enrich`. A scan not
  cancelled runs `enrich(false)` as it ends where `can_enrich` (online *and* a reference) allows; the
  lookup polls at `SCAN_POLL` and reloads every `POLLS_PER_RELOAD` polls. Online off under a run keeps
  the reference and only stops `enrich` starting (`has_reference`); the shared client hears the switch
  at once (`online.md`); `AnalysisModel::reach` stops recognition asks while off.
- **A file changed under a root while the window runs is followed on its own; one taken away is
  forgotten at once, not scanned for.** `resonate_library::RootsWatch` = recursive inotify watch (via
  `notify`):
  - *Gone* (audio file or folder removed): handed out by `taken_away` once `GONE_QUIET_FOR` (250 ms)
    has passed, never while a change under the roots is settling (a rename is a path gone and changed
    at once; the scan follows the file instead of forgetting and recounting it).
  - *Changed* (audio file or sheet made, written or renamed in; a folder made or renamed in; a sheet
    taken away): `settled` hands the root out once quiet for `ROOTS_QUIET_FOR` (2 s), or once its first
    change has waited `DEFERRED_AT_MOST` (5 minutes)
    (`a_folder_that_never_goes_quiet_is_handed_out_once_it_has_been_deferred_long_enough`). An
    overflowed inotify queue notes every root. A catalog inside a root cannot trigger the scan writing
    it; a vault inside one does, and the scan steps past it.
  - `watch_the_roots` looks every `ROOTS_LOOKED_AT_EVERY` (250 ms), rebuilding the watch wherever
    `Library::roots` moved, over roots present as directories (`RootsWatch::taking_over` carries what
    the old watch heard and had not handed out; a watch that cannot be laid leaves the old one
    standing). What is gone goes to `Library::forget_the_gone` (takes the `Walk` guard, deletes rows
    whose file is missing, sparing a vaulted and a delivered row as the scan's prune does, and rows on
    an absent volume); a forget refused by `Error::AlreadyWalking` or failed is kept and retried. A
    settled root with the work slot free runs the incremental scan, `Prompted::OnItsOwn`. A non-UTF-8
    path is passed over, not failing its batch.
  - A watch that cannot be made is a warning; one that cannot cover a root
    (`RootsWatch::leaves_a_root_uncovered`) is laid again every `UNCOVERED_ROOTS_TRIED_AGAIN_AFTER`
    (5 minutes); proved past the limit by
    `a_root_past_the_inotify_limit_is_said_uncovered_and_the_root_watched_before_it_still_hears`
    (reruns its binary under `unshare --user` with `max_inotify_watches` at eight).
  - **Closed-window changes are caught up on open; a missing root is watched once it appears.** The
    first look that has read the roots runs one incremental scan of every root naming none, so an
    unmounted one is skipped by `is_there` instead of failing the scan (`RootNotADirectory`).
    `Watching` watches only roots that are folders now, keeps the rest as `absent`; one turning up
    later rebuilds the watch and is owed its own scan.
- **A browse pane holds a prefix of the listing, but is as long as the whole match.**
  `LibraryModel::reach` = rows the four browse lists are read to, `PAGE` (2 000) to start. Each
  `uniform_list` counts the catalog's match (`albums_listed`, `artists_listed`, `listed_rows`), so the
  scrollbar, `End`, `ctrl-a` and the page keys reach the last row; an unread row draws blank. The
  processors call `reach_further`: once a list has drawn within `LOOK_AHEAD` (200) of what it holds
  *and* holds all it asked, `reach` grows to cover what was drawn (`reach_covering`). **A page read
  reads only what lies past the rows held** (`grown`, `Wanted::ThePage`, `Asked::held`): `grew` adds the
  read only where its first row is the row held last (`Grew::Renewed`); otherwise `Grew::Moved` and the
  lists are read whole once (`page_moved`), so an offset read never duplicates or skips a row.
  `LOOK_AHEAD < PAGE` is a `const` assertion; `set_query` and `select` reset to one page. A row pressed
  past what is held plays through `play_the_listing_from`'s whole read
  (`a_page_read_past_what_is_held_is_added_to_it_only_where_it_carries_on_from_the_last_row`).
- **A count is what the query matches, not what the window holds.** `Library::albums_counted`,
  `artists_counted`, `measured` answer over the whole match (the first two share scoping with the
  listings via `narrowed_onto`). `Measured` = rows, total length, lossless count (from `tracks.codec IN
  (…)` built off `Codec::ALL` filtered by `is_lossless`). The sidebar's figures and tracks summary read
  these, not `Vec::len`; `measured` honours a limit it is given (a saved query's cap), the window passes
  `limit: None`.
- **A gesture over "every track listed here" reads the whole listing first**:
  `RootView::with_everything_listed` reads the unlimited `TrackQuery` from `listing_whole` on the
  background executor; tracks-heading *Play*/*Shuffle* act on all the search matches, not the loaded
  window.
- **A scope belongs to the tracks pane; the sidebar counts the library.** `browsed` reads the albums,
  artists, tracks the search allows, plus a *second*, narrower track listing only where an album or
  artist is scoped. `LibraryModel::tracks` = the first; `LibraryModel::listing` = what the tracks pane,
  heading, summary, *Play*, *Shuffle* draw from, so sidebar counts hold however deep the listener goes.
  A scoped album hero draws the owner on its own line via `RootView::opens`; the scoped cover has
  `COVER_HINT` and `RootView::magnify` (a portrait does not: `Magnified` is an album or a file).
- **Album rows = catalog rows merged with the release's missing ones.** `LibraryModel::rows` =
  `Arc<[ListedRow]>` from `album_rows` when the selection is an album: `ListedRow::Held` indexes the
  tracks listing, `Missing` the release rows; `Disc` heads each disc's run (two or more discs, seat
  order only); `NotHeld`/`Found` sit below an artist page's rows. Sorted by `(disc, position)`; seat
  order while the pane's sort is the album's own (`Relevance`, `AlbumThenTrack`), reversed where it
  reads backwards; any other sort draws held rows in that sort, then what the album lacks (`arranged`).
  **What plays is what is drawn:** click/Enter go through `LibraryModel::played_from`; *Play*, *Play
  next*, *Add to queue*, *Add to playlist* put the whole listing through `AsDrawn::ordered`.
  - **A missing row on an album page is a download press.** `Beside::AnAlbum` makes an unheld release
    row clickable when it has no want (`LibraryModel::want`); *Get the rest* =
    `want_missing_tracks`: every unresolved release row wanted in one transaction, one provider poll.
    `unheld_row` draws faint in the same eight cells; `controls_place_of` takes the held row's controls
    width (`TRACK_CONTROLS` 3, `TRACK_ADD_CONTROLS` 2, Missing `ROW_CONTROLS` 4). One mark: `Icon::Want`
    → `want` (bg, then `wants_landed`: `list_as_downloads`, `fetch_or_say_nobody_can`, reread) or
    `Icon::Wanted` → `unwant` (via `edit`); `Loaded::wanted` maps `ReleaseTrackId` to `WantId`. **A want
    says how it is going where pressed:** `WantStanding` from the shelves; `fetching_want` reads a
    `Fetching` (*Queued*, *Downloading*, *Retrying*, *Gave up*, *Downloaded*), drawn in
    `fetching_colour` in the format cell. No provider set up: toast *No provider is set up*
    (`has_a_source`). A grid caption and scoped heading do not count what an album is short of: Missing
    pane, inline `unheld_row`s, sidebar figure and the artist heading each say it once.
  - The info mark at row end, where `record_of` has something to say, opens a card anchored at the
    press: Released, Format, Label, Catalogue number, Barcode, Country, Kind, every service link. With
    a release group and a reachable reference, *Other pressings* asks `Reference::release_group` on bg
    (`ask_for_pressings`, `pressings`), first `PRESSINGS_SHOWN` (12), the one in use badged *IN USE*;
    pressing another = `take_pressing`. **A track is placed on a release from its own menu:** *Place on
    a release…* reads the recording via `releases_it_could_sit_on` (`RELEASES_OFFERED` 10), press =
    `place_on`. **An album nothing matched has the card too**: *Find the record* asks
    `Reference::find_release` by words and any barcode/catalogue number from tags
    (`ask_for_releases`), press = `take_pressing`. Where matched, the card ends in *Not this record*:
    two presses (arming held on `OpenedRecord::Album` so any way the card closes lowers it) =
    `forget_the_match`. Escape closes the card after a magnified cover, before a toast; leaving the
    album closes it; its scrim occludes.
  - Artist hero: `profile_line`, `heard_on`, genres from the info mark (`kit::tag`). Where
    `ArtistDetail::releases_unheld` > 0 a `Tone::Ghost` button *N releases not held* (plus *, M more
    unread*) under `UNHELD_HINT` opens `Pane::Missing`; with unread releases and `can_enrich`, *Read the
    rest* = `read_the_rest_of` (`library.md`). **An artist's services are capped at `SERVICES_SHOWN`
    (4)** (a release's card and the genre card are uncapped). **Each service is the link it was named
    from**: `service_names` keeps the first URL per service beside its `Service::title` (omitting
    `Service::Other`); `heard_on` and the record card draw one pressable name per service, *Open on …*,
    handing that exact URL to `cx.open_url`; the press stops propagation and ignores a right click.
- **The artists pane is a grid or a list; the heading chooses.** `resonate_core::ArtistsDrawn` = `Grid`
  (default) or `List`. `artist_grid` shares the albums grid's `grid_width`/`grid_columns` with round
  `portrait_frame`s read at `Portrayed::InAGrid` (`kit::avatar_at` where none is held). Persisted:
  `RootView::draw_the_artists_as` writes `Setting::ArtistsDrawn` (`artists-drawn`) and
  `ResonateApp::artists_drawn`. Reach keys serve both.
- **An artist's page is its albums or its tracks, one at a time, from two tabs.** `artist_tabs` =
  `kit::segmented` *Albums*/*Tracks* with counts; `RootView::artist_shows` (`ArtistShows`) is kept for
  the run and reset to `Records` whenever `RootView::opened` opens an artist. *Albums* =
  `artist_records`: every album the artist owns or plays on, a wrapping grid of `album_cell_captioned`
  (`Caption::Beside`: year and track count, owner only where somebody else's), scrolling under an id
  keyed by the artist. *Tracks* = the ordinary listing, the only tab with the sort icon.
  `ArtistShows::within` answers *Tracks* for an artist with no album. *Play*, *Add to queue*, *Play
  next*, *Add to playlist* read the whole listing either way.
- **What an artist's discography holds that the library does not is on the artist's page, greyed.**
  Rides in the load (`Library::albums_not_held_by`, `songs_not_held_by`; `library.md`). Selecting the
  artist also starts `LibraryModel::learn_the_songs_of` where Online is on, reading songs of unheld
  releases the lookup pass has not reached; it reports through a `Learning` (`PageLearning`) as each
  group lands, the window re-reads per drained batch, and opening another artist replaces `_learning`
  (the walk sees `abandoned` before its next request). *Albums* follows the held cells with
  `not_held_heading` (*Not in your library · N releases*) and an `album_not_held_cell` per release
  (front via `released_cover_with_group` at `UNHELD_COVER` 0.45, from `Library::unheld_cover` before
  the archive is asked live; title in `theme::faint()`). Pressing one = `land_album_not_held`:
  `Library::open_album_uncovered` on bg (release read whole, landed as an album with release rows and no
  want), then the album opens where each missing song is pressed to be wanted; `want_album` serves a
  link to an album not held; `fetching_album` reads its songs from `Downloads`. *Tracks* follows the
  held rows with `ListedRow::NotHeld` (*Not in your library · N songs · press one to download it*) and
  a `found_row` per song (press = `want_found`) via `restate_the_listing`/`beyond_the_listing`, which
  answers nothing until the listing is whole; a page query narrows them via `still_answering`.
- **Missing pane = what the catalog knows it is short of, headed by run, one half at a time.**
  `Pane::Missing` under `Section::Collection`; sidebar count `Missing::tracks`, hidden at zero. Two
  tabs, `missing_tabs` (*Tracks*/*Releases*, counts), `RootView::missing_shows` (`MissingShows`, kept
  for the run); `MissingShows::within` hands over to the half holding something; the artist heading's
  *N releases not held* opens on *Releases*. Each tab = one `uniform_list` over
  `missing_track_rows` or `unheld_release_rows` (`Arc<[MissingRow]>`), a heading wherever the key
  changes: artists' half `headed_by_run`, albums' half `headed_by_album_and_disc` (a disc heading only
  where the album's run spans more than one disc: `headed_by_disc`; eyebrow *DISC N*). **A run is a
  card drawn a row at a time (every `uniform_list` row is one height):** `Place::of` (`Head`, `Within`,
  `Last`) and `in_a_card` draw heading on `raised` with rounded top corners, rows on `surface` with
  hairlines, last with rounded bottom corners; the gap between cards comes out of the bordering rows,
  heading and last each `HALF_BETWEEN_CARDS` (6) shorter than their row
  (`a_run_is_one_card_opened_by_its_heading_and_closed_by_its_last_row`). `run_band` = album cover or
  artist portrait (`kit::avatar` where none), name via `opens`, owner muted, count ending where lengths
  end. Track row = `unheld_row` under `Beside::ARun` (no format or plays cells); release row =
  `release_row`: title muted, kind a `kit::badge`, first release year in the length column, no press.
  `Loaded` reads `missing_tracks` and `unheld_releases` under `MISSING_AT_MOST` (5 000) and
  `missing_counted` on every load, so subtitle and tabs count the catalog's; **a list cut short of its
  count says so** (`listed_short`: *5000 of 7214 tracks listed; type to narrow to the rest*). **A row
  is dismissed with its ✕** (`dismiss_missing`, `dismiss_release`, via `edit`); heading's *Bring back N
  dismissed* = `bring_back_dismissed`, toasted (`library.md`). Empty: `kit::empty` under
  `Icon::Missing` (*Nothing missing matches.* while narrowed).
- **A song asked for is followed in the sidebar, never told in a toast.** `downloads.rs`: `Downloads`
  holds a `Download` per found song pressed (the `Found`, `WantId` once landed, when queued, a
  `Fetching`); `LibraryModel::want_found` writes it: `Landing`, then `Queued` (`NoProvider` where
  `Sourcing::providers` registers none, `Unwanted` where landing failed). `Downloads::followed` reads
  each shelves load's `WantStanding`s: delivered = `Downloaded`; tried since queued with nothing offered
  = `Retrying { tries, at }`; given up = `GaveUp`. **A download says two things while underway: *Looking
  it up…* and *Downloading…*.** `Landing`, `Queued`, `Retrying`, `Unreached` and a `Downloading` with
  nothing yet received read *Looking it up…* (never an attempt count or *No match* between polls);
  `downloads::saying_while` reads *Downloading…* once `Fetched::is_arriving` (`LibraryModel::fetched`),
  on the panel and the song's own row (`told_found`, `told_want`). Sidebar `summed_up` says one of the
  two with *· N left* while more than one song is underway, else *N downloaded · N not downloaded*
  (`a_download_says_it_is_looking_the_song_up_until_bytes_arrive_and_downloading_after`,
  `the_sidebar_says_looking_it_up_or_downloading_and_how_many_songs_are_left`). Only an ending speaks
  otherwise: *Downloaded*, *Not found*, *No provider is set up*, *Couldn't add it*. A poll ending with a
  provider refusing or late (`PollStats::refused`/`late`) sets `LibraryModel::providers_unheard`; while
  set, a queued or past-due retrying download reads `Unreached { attempt }` (`providers.md`;
  `Fetching::while_polling` is the one reading). **Standings are re-read while the poll runs**
  (`Followed` weighs `PollStats` each `SCAN_POLL`, rereading shelves every
  `POLLS_PER_REREAD_WHILE_ASKING` (5) ticks that moved). **The list outlives the window**:
  `Downloads::restored` relists every want still underway
  (`what_was_still_underway_comes_back_after_a_restart_and_nothing_finished_does`).
  - Sidebar `RootView::download_status` (`Icon::Download` in the accent while any is underway) opens
    `downloads_over_the_app`, a panel `DOWNLOADS_PANEL_GAP` (8) beside the sidebar above the playback
    bar; a press outside closes it, and the row's own click right after is ignored for
    `DOWNLOADS_TOGGLE_PRESS` (600 ms). Each song: title, artist, state in `browser::fetching_colour`
    (accent downloading, `done` downloaded, `failure` given up/unprovided/unwanted, `muted`
    landing/queued/retrying), clipped lines beside a `flex_none` group of marks. The text opens the album
    the want is on (`downloaded_album`); `Icon::Play` plays a downloaded song (`downloaded_track`);
    `Icon::Redo` where `can_be_asked_again` (`want_found` once more, making the want due at once,
    `library.md`); ✕ on a finished one; *Clear finished*. `Fetching::can_be_cancelled` (not landing) gets
    `Icon::Stop`: `cancel_download` takes it off the list, withdraws its want (`Library::unwant`) and, if
    the poll is asking for that very want, cancels the poll. Escape closes the panel (after a menu,
    before a toast); it is not modal. A self-started poll joining while the list holds anything tells no
    toast. The found row says the same where it stands (format column draws the `Fetching`, want mark
    greyed with the state as hint; a press only where nothing is asked for yet or `can_be_asked_again`);
    section heading *Press a song to download it*
    (`pressing_an_ncs_song_found_on_musicbrainz_wants_it_and_asks_the_providers_for_it`).
- **A song in the queue is taken out from its menu wherever listed**: *Take out of the queue* while
  `RootView::is_in_the_queue` finds a row with the same location and span; `take_out_of_the_queue`
  drops every such row through `drop_rows`.
- **A song leaves the disk only through a dialogue saying where it goes.** *Delete from
  disk…* (`Icon::Delete`) = `RootView::ask_to_delete` with a `Deleting` (track, title, artist, whether a
  cut); `deletion_sheet` (`theme::confirm_width`) names the song, says the file goes to the trash and the
  song leaves the library, that a cut takes its whole file and every cut from it. *Cancel*,
  escape, press outside keep the song; only *Delete* (`Tone::Destructive`) calls
  `LibraryModel::delete_track` = `Library::delete_tracks` on the edit chain. No key confirms, `enter`
  included. While open it is `something_stands_over_the_pane`
  (`deleting_a_track_asks_first_and_takes_its_file_only_once_confirmed`).
- **A search is a page of its own; what the library lacks is never buried under what it holds.**
  `views/search.rs`. `RootView::search_in_front` answers a `SearchShows` wherever the box holds words,
  the pane in front is Albums, Artists or Tracks, nothing is scoped and no songs are being added to a
  playlist; `content` then draws `search_pane`. Heading (`search_heading`): *SEARCH* over the quoted
  words, a summary (*12 songs · 3 albums · 1 artist in your library · 18 songs not in it*), actions
  (*Search the words as typed*, *Try again*, *Sung in*, *Save this search*, sort, *Shuffle*, *Play*),
  *Reads* chips, tabs with count pills: *Top results*, *Songs*, *Albums*, *Artists*. **A tab counts the
  library's and what was found beyond it together** (*…* while MusicBrainz (MB) is asked). *Songs*,
  *Albums*, *Artists* are the three panes themselves (they take `search_heading`); choosing one is
  `show_in_the_search`, keeping its reach, sort and scroll; a sidebar press on one while searching is
  that tab (`SearchShows::in_place_of`). *Top results* (`top_results`) = one scrolling page: matching
  artists as a strip of portraits at `ARTIST_AT_THE_TOP` (72 px), first `SONGS_AT_THE_TOP` (5) songs,
  first `FOUND_AT_THE_TOP` (6) songs not in the library, albums as a strip, then *Albums not in your
  library*; each section headed by its name and, where more matched than shown, *See all N*.
  - **A tab lists what was found beyond the library under what the library holds, never in a tab of its
    own.** *Songs*: `LibraryModel::rows` holds held rows, a `ListedRow::NotHeld` heading and a
    `ListedRow::Found` row per song once the whole held listing is loaded (`beyond_the_listing`); enter
    on a found row = `want_found` (`found_at`). *Albums*/*Artists* keep their grid or list and end in a
    `flex_none` strip, *Albums not in your library* (`album_found_cell`, `AlbumFound`) and *Artists not
    in your library* (`artist_found_cell`); with nothing at all matched the pane says *Asking
    MusicBrainz…* or *MusicBrainz could not be reached* (`nothing_beyond`). A found album's cell = an
    artist page's release-not-held cell (`unheld_album_cell`), press `land_album_not_held`. **An artist
    the search names that the catalog does not hold is a cell of its own**: `artists_found` =
    `ArtistFound`s `show_answer` weighed beside the songs (`Library::unheld_artists_among`, off the
    credits MB answered with: no request of its own), read only while `found_for` is the query; press =
    `land_artist_found` (`Library::open_artist_found` on bg). While its id is in
    `albums_opening`/`artists_opening` a found album reads *Opening…* in the accent and a found artist
    carries *Opening…* under its name. **A song not held names its artist as a link too**:
    `unheld_row` draws a `Found`'s artist via `performed_by` (`Performer::Held` opens that artist;
    `Performer::Elsewhere` → `open_artist_found`; none keeps a plain name); the press stops at the name
    and never wants the song
    (`pressing_the_artist_of_a_song_found_on_musicbrainz_opens_the_artist_and_wants_nothing`).
  - A search always begins on *Top results*, read where the box goes from empty to words
    (`RootView::open_the_search`): from Albums, Artists or Tracks the pane stays; from any other pane
    the window goes to Tracks showing everything
    (`a_search_begun_on_any_pane_opens_the_top_results`). So under *All tracks* `LibraryModel::rows` is
    empty until a search finds songs, and `LibraryModel::elsewhere` answers the MB half alone as a
    `Beyond`: `Elsewhere(n)`, `Refining(n)`, `Asking`, `Unreached` (`elsewhere_standing`). Keyboard:
    `reachable` reaches the top songs (or the found rows where none is held) and the *Songs* tab's rows
    (`songs_not_in_the_library_stand_on_the_first_page_however_many_it_holds`,
    `an_album_found_elsewhere_stands_under_the_albums_held_and_in_the_top_results`).
  - A search lists no rows a held album is short of (they stay in the Missing pane and the artist's
    page), only songs of releases not held at all (`songs_kept_for`). A found song's `unheld_row` has a
    `Beside::ASearch` cover column (`Sleeve::Released`), which `LibraryModel::released_cover` asks the
    reference for on bg while Online is on: `FETCHES_AT_ONCE` (6) at a time, decoded on the `Drawer`,
    held in `released_covers` (`RELEASED_COVERS_HELD` 512: more than a discography draws, else the grid
    evicts what it is still asking for). Want mark = `want_mark` over an `Asks`: a found song calls
    `want_found`, landing its release and wanting the row on bg, mark greyed meanwhile.
    `restate_what_was_found` passes over a found song whose want holds a track the held listing draws
    (`is_listed_as_held`), so it is listed once, as the held track, once the catalog's next read carries
    it (`a_found_song_that_landed_is_listed_once_as_the_held_track`). **A found song's whole row is that
    press** (`found_row`, `FETCH_FOUND_HINT`); the mark's own press finds the recording already
    `wanting`; while asked for, the row takes no press. `wanting` holds a task per recording.
  - **A search reaches MB as seldom as it can and never leaves the listener looking at nothing.** MB
    answers one request a second, so turns go only to what is still being typed:
    - *Asked once per spelling*: `answers`, a `Recent` of `ANSWERS_HELD` (128) keyed by `songs_asked`
      (case, spacing and grammar the ask ignores share a key), holds the reference's matches, not the
      `Found`s (`Library::unheld_among` weighs those again each time)
      (`words_searched_again_are_answered_from_memory_rather_than_asked_twice`).
    - *One ask in flight, the latest text winning*: `ask_elsewhere_after` waits `ASKED_ELSEWHERE_AFTER`
      (450 ms) behind the keystroke, only where Online is on, a reference is there, the search is not
      scoped to an album or artist (`a_search_inside_an_artist_asks_musicbrainz_nothing`) and
      `songs_asked` names words worth it; then `reach_out`: songs (`songs_reached`, drawn at once as
      `Shown::InPart`) then albums (`find_albums`). While a chain is out (`reaching_out`) the text is
      `owed`; a chain whose text was typed past drops its albums
      (`a_search_typed_past_does_not_ask_musicbrainz_for_its_albums`). Only a whole answer is remembered
      (`found_in_part`); a refused albums half leaves the songs drawn and is asked again
      (`songs_found_stand_where_musicbrainz_refused_the_albums_which_are_asked_for_again`). Enter
      (`ask_elsewhere_now`) skips the settle.
    - *Narrowed while asking*: `found` stands in as `narrowed` (`still_answering` the new words) under
      `Beyond::Refining`; kept songs and albums and MB's albums and artists narrow likewise
      (`albums_still_answering`, `artists_still_answering`), so counts and strips do not fall to nothing
      per key (`songs_found_for_fewer_words_stay_listed_while_more_are_asked_for`,
      `albums_found_stay_listed_while_the_words_are_edited`).
    - *A failure says so*: a refused or unreachable request is not remembered; its text is
      `unreached_for` and `elsewhere` answers `Beyond::Unreached` with *Try again*
      (`ask_elsewhere_again`), beside the songs still listed
      (`a_search_musicbrainz_did_not_answer_says_so_beside_the_songs_kept`); a failure for a text typed
      past is a debug line only. The answer drawn is kept against the text it answered (`found_for`).
    - *What the catalog already knows comes first*: `Library::songs_kept_for` (songs of library artists'
      unheld releases, learnt in the lookup pass; only where Online is on) rides in the load with its
      text (`kept_for`); `kept_before_the_rest` puts them first and drops an MB answer naming the same
      recording or the same folded title and credit
      (`songs_kept_from_a_discography_come_first_and_musicbrainz_does_not_repeat_them`). Albums likewise
      (`albums_kept_for`, `albums_kept_before_the_rest`).
  - A search whose plain words a held track sings is offered as `lyrics:"…"` (`Library::sung`): *Sung in
    N tracks* in the heading's actions and in a pane's empty state, as a `search_instead`.
- **A search's reading is said under its words; the words as typed are a press away.** With
  `LibraryModel::meant` holding a `Meant` (`library.md`): the heading draws *Read as “You F O” by Stela
  Cole* in accent, *Reads* chips show the scoped search run, matched runs light by it, *Save this
  search* saves it, *Search the words as typed* is `search_as_typed` (literal until the box changes).
- **A pasted song or album link is downloaded, an artist link opens the artist; none is searched.**
  The search field is `catching(is_a_followed_link)`: a taken paste is emitted as `Caught`, not
  inserted; `ctrl-v` away from any field is `PasteAway`. `RootView::follow_link` calls
  `Library::follow_link`/`follow_album_link`/`follow_artist_link` on the background executor via
  `LibraryModel::follows_links` (the reference while Online is on, else a toast to turn it on). `followed`:
  song/album already held: toast; song found: `want_found`; album not held: `want_album`; album held
  short: `want_missing_tracks`; each lands in the downloads list as a press would. Box untouched,
  nothing searched; an unnamed link gets a toast. An artist link opens the page (held at once, else
  MB-named via `land_artist_found`); Deezer, Spotify, Apple Music, TIDAL, YouTube, SoundCloud, Bandcamp
  and Amazon Music artist links are caught too, followed by the artist MB files their page under. **A
  Deezer, ListenBrainz, Spotify, Apple Music or YouTube playlist link fills a playlist of what is held
  and wants the rest**: `followed_a_playlist` wants every found song, then `Library::fill_playlist_named`
  on bg puts the held songs into the playlist of that name (made where none is; an existing one gains
  only what it lacks), opens it and toasts (`playlist_told`); where nothing is held no playlist is made
  (`a_playlist_link_pasted_into_the_search_wants_every_song_it_holds_that_is_not_held`,
  `a_song_link_pasted_into_the_search_is_downloaded_and_leaves_the_box_as_it_was`; links read:
  `library.md`; service asked: `online.md`).
- **The search box narrows the Missing pane; its halves answer differently.** Both reads take the
  browse query (SQL: `library.md`). Missing track: narrowed by the *album* it is short from, via the
  catalog grammar. Unheld release: its own row only, by the fold of title, kind, first release date, or
  artist. Nothing matching: *Nothing missing matches*, no lookup offered.
- **Playlists pane = an index plus one opened playlist, not a `Selection`.** A playlist is
  `LibraryModel::opened`; its entries ride the same `Loaded` snapshot as albums, artists, tracks, roots.
  Rows enter one via `RootView::hold_for_a_playlist` (picker: a queue row's + and the queue's *Save as
  a playlist*); index/opened-playlist + enter track-browsing mode for the target, a track row's + adds
  directly, *Done*/Escape returns. The picker's listing rides the snapshot (`Library::playlist_lists`:
  unnarrowed, list-playlists only), so it reads nothing on its opening frame and `adding` carries the
  `Held` alone. `Offered` steps over the source playlist by position, so `uniform_list` draws
  `theme::PICKER_ROWS` (9) with no per-frame copy. `RootView::show_playlist` opens one with the
  `UniformListScrollHandle` `left_at` holds for its id; `set_query` installs a fresh handle when a
  keystroke narrows differently; `forget_what_has_gone` drops a handle once the listing stops naming
  its playlist (skipped while a search narrows). The index's sort icon sits with icon-only Import and
  New playlist in the heading's right actions and reveals the ORDER and READS choices; none of this
  survives the run.
- **A playlist pressed from the index is read off the window's thread.** *Play*, *Shuffle*, *Next*
  and *Last* (index row, cover, menu) go through `RootView::with_playlist_entries`: an opened,
  unnarrowed playlist answers from the snapshot at once (`LibraryModel::held_entries_of`), any other is
  read by `Library::playlist_entries` on the background executor, a smart one running its query there,
  and a failure is a toast and a log line. `play_the_playlist`, `shuffle_the_playlist` and
  `queue_the_playlist` do their sending in the continuation, so shuffle is turned off or on only when
  the rows are in hand
  (`a_playlist_that_is_not_opened_is_read_off_the_windows_thread_and_an_opened_one_is_at_hand`).
- **One `Field` names every playlist.** `RootView::name` serves the pane's new-and-rename row and the
  picker's *or a new one*, one open at a time (`name_a_playlist` closes the picker, `hold_for_a_playlist`
  the naming row). It emits `Submitted` on enter; `RootView::name_given` decides from whichever is open:
  create, create-and-add, rename, or save/revise a search. The `Naming` variant (`Query` under
  *tracks*, `New`/`Rename` under playlists) picks the heading drawing the row (one entity can't be in
  the frame twice). Order and cap ride beside the name as chips (a saved query = search, order, row
  cap). *Edit search* = `RootView::revise_search`. The Online card's fields follow one pattern:
  `RootView::contact` is filled by `Field::hold` (the saved contact is in the box when the window opens,
  put back trimmed on enter); `contact_given` stores `Setting::Contact`, moves the global, reports it
  is sent from the next request (`online.md`); press outside = `leave_contact` (gives typed text if it
  differs), escape = `put_back_contact`. The field losing focus any other way (tab: `field::Blurred`)
  takes what was typed through `contact_left` and kin without taking the caret back to the window
  (`take_the_contact` is the one giving; `contact_given` adds the refocus); the typed folder adds
  itself the same way and escape clears it first
  (`tab_out_of_an_online_field_gives_what_was_typed_without_taking_the_caret_back`). Same pair for
  AcoustID key, AudD and ListenBrainz tokens, Subsonic and TIDAL accounts, organise layout (`layout_left`
  keeps a valid one, reports a refused one)
  (`a_contact_typed_and_left_with_the_pointer_is_kept_and_one_left_with_escape_is_put_back`), ahead of
  the naming row in `dismiss_search`.
- **A run of rows is dragged where it goes; the keyboard reaches one before moving it.**
  `views/reorder.rs`: `Shift` names the list edited (`Queue`, `Playlist`, browse `Listing`); `Step`
  up/down and `Step::landing` are the one arithmetic behind arrows, drop and keys. A row is drag source
  and drop target; dropping a span on row `to` = the arrows' `Command::Move` (never a swap). Payload
  `Carried`; `Ghost` = first row's title plus how many more ride. `RootView::reach` is a `Reach`
  (`Shift`, anchor, the row moved to; lives for the run, and a non-queue reach is let go of when the
  rows under it are arranged another way: `Arrangement` = selection, sorting, playlist order and
  reading, opened playlist's order, search sort, compared by `let_go_of_a_reach_the_rows_left` on
  every library change and sort setter; `opening_an_album_lets_go_of_the_row_reached_in_the_listing_before`). `up`/`down` move and collapse it (queue opens
  on the playing row, playlist on its first); `shift-up`/`down` grow it; `home`/`end`; `pageup`/
  `pagedown` by `rows_a_page`; `ctrl-a` every row; `shift`-click every row between; `alt-up`/`alt-down`
  move what is reached; `enter` plays its first row; `delete` removes reached rows via `drop_rows`
  (gated on `Rows::are_edited` in a playlist); `backspace` on a reached row drops it too
  (`drop_reached` answers whether it acted; the query is edited only if not, unless a type-ahead jump
  put the reach there). `RootView::acting_on` is what every row gesture asks: the reach if the pressed
  row is inside it, else that row alone. Drawn as a 2-unit accent edge plus faint wash; every
  reorderable row reserves the edge in `theme::UNMARKED`; saved queries draw none. A drop does not ask
  `acting_on`: `reorder::movable` takes the row it stands for beside the `Carried` it would hand over.
- **A track or album is dragged out of a listing into the queue or a playlist.** Track rows and album
  cells carry a `reorder::Lifted` (`Lift::Tracks`/`Lift::Album`) through `reorder::liftable`; a row
  standing in the reach carries every loaded row of the reach as a `reorder::Run`: the page's own
  `Arc<[Track]>` and a `Span`, never a copy, so a 2 000-row selection costs a row an `Arc` clone per
  frame, and the ghost's title is worked out only when a drag begins
  (`a_lifted_run_shares_the_page_it_was_lifted_from_and_is_named_by_its_first_song`). `reorder::takes_a_lift` makes targets of
  a queue row (`Placement::At`), an opened playlist's row (`LiftedTo::PlaylistAt`:
  `Library::insert_into_playlist`, one undoable edit), the queue pane's body and empty queue
  (`Placement::Queued`), the playback bar's queue button, and a list playlist's index row, card and
  pinned sidebar row (a saved search takes none). `RootView::lifted_into` lands tracks at once and
  reads an album in album order on bg first. The innermost target takes the drop
  (`a_track_dragged_onto_the_queue_button_is_queued_and_an_album_onto_a_pinned_playlist_fills_it`,
  `rows_put_into_a_playlist_at_a_place_land_there_and_one_undo_takes_them_out`).
- **A drag scrolls the list; a held pointer keeps it scrolling.** `reorder::follows_a_drag` puts an
  `on_drag_move` on the div wrapping each `uniform_list`, handing `RootView::creep` a `Creeping` while
  the pointer is within `DRAGGING_EDGE` (36) of an edge. gpui reports drags only on movement, so
  `creeping_on` repeats every `DRAGGING_STEP` (60 ms) until `creeping` clears or
  `App::has_active_drag` says the drag is over; one task serves both ends and panes and alone clears
  the cell.

- **Settings pane: a rail of categories over a column of sections; one table says what a section is.**
  `views/settings/` is a folder. `find.rs` = vocabulary: `Category` (Output, Processing, Equaliser,
  Library, Filters, Online, Recognition, Scrobbling, Sync, Desktop, Appearance, About) and `Group`, one per setting, carrying category,
  title, hint, *also*-called words, the `SettingKey`s putting it back. Nothing else enumerates sections:
  `Category::groups` filters the one table, `mod.rs::group` dispatches a `Group` to its drawing method;
  tests hold every group to one category, a unique title, its own hint, no key claimed twice. Rail =
  `theme::settings_rail()`; `RootView` remembers the open category for the run; body = a column of
  `kit::section`s (header strip: title, revert mark, info mark). Body scrolls under
  `RootView::settings_scroll`; `show_settings` resets it on category change (gpui keys scroll offset by
  element id).
- **Filters** (groups `MusicExtensions`, `MinimumLength`): one wrapping multi-select of `kit::chip`s with
  a checkmark when included (the slot keeps its size when cleared), all selected by default; *Select
  all*/*Clear selection* greyed where they would change nothing. Minimum: seconds field, 0 to 600,
  applied on Enter or leaving (a positive minimum excludes tracks of unknown length); a refused number
  stays in the field and toasts; Escape restores the held value.
- **The library's folders are one box, and the primary one is chosen in it.** `Group::Folders`
  (*Music folders*, Library category) lists every root as a card: a radio, the folder, a
  `PRIMARY` badge on the chosen one and the forget mark. Pressing a card's radio or name makes that
  root `music-folder` (checked off the window's thread, then `set_the_music_folder`); pressing the
  primary again chooses none. A `music-folder` that is no root itself (inside one, or waiting to be
  added) is drawn as a card of its own with a mark that clears it. Forgetting a root holding the
  primary clears the primary. *Add folder…* and the typed field add roots; the *File dropped songs
  by the layout* switch ends the box, and the group's mark puts back no primary and the switch on.
- **The Resonate server key is a group of its own** (`Group::ResonateKey`, Library category), one
  masked `Field` that is `TidalAccount::HifiSecret` in the same array as the TIDAL fields (so focus,
  leaving and putting back are the TIDAL fields' one path) and drawn apart from them; the TIDAL
  group's reset and `tidal_given` leave it out (`providers.md`).
- **Files dragged in from outside are weighed while over the window and copied when dropped.**
  `views/dropping.rs`. gpui makes a file drag an `ExternalPaths` drag, but only the drop target hears it
  end, so the root's `on_drag_move::<ExternalPaths>` sets `RootView::incoming` (paths, and
  `resonate_library::weigh`'s read of each) and starts `watching_the_drag`, a 100 ms timer clearing it
  once `App::has_active_drag` is false. **Nothing on a drag or a frame touches the disc on the render
  thread**: `weigh_the_drag` reads on the background executor (`Verdict::Weighing` until it lands); the
  music folder's standing (`RootView::music_folder_is_there`, a `FolderStanding`) is looked at on bg at
  most every `FOLDER_LOOKED_AT_EVERY` (2 s) while something draws it, and once more off-thread before
  `land_the_drop` acts. While `incoming` is set, `drop_overlay` draws over the window and is the
  occluding `on_drop` target: a scrim, a dashed card listing up to six names each marked *copied*, *cue
  sheet, copied*, *beside a song, copied with it*, *not audio, left out* or *not there*, and the
  destination. `verdict` is the one decision overlay and drop share: `Ready`, `NoFolder`, `FolderGone`,
  `NothingToTake`, `Busy` (the last four draw in the failure colour and toast when dropped; no
  `music-folder` also opens Settings on Library). Ready: `take_in` into `ResonateApp::music_folder`;
  `copying_pill` draws progress and *Stop*; `told_of` toasts; where anything was copied the root is
  scanned via `add_roots`, with `file-dropped` on the landed songs first handed to `file_once_scanned`
  (`library.md`'s *Taking files in*). The handle lives in `ResonateApp::copying`; at event-loop end
  `run` winds it down (`wound_down`: cancel, wait up to `WIND_DOWN_WITHIN` 10 s) before the library's
  own pass. A file being copied is `.<name>.<pid>.resonate-part` until renamed into place whole.
- **Sync is a category of its own** (`Category::Sync`, `Icon::Sync`; `views/settings/sync.rs`,
  `sync.md`), five groups:
  - *Device sync*: the switch and a standing card (a tinted mark, `done`, accent, `failure` or
    muted by `Mood`, a headline and a line under it read off `SyncShown`; *Sync now* while syncing),
    and how many songs are on their way.
  - *Pairing* (keyless): a `pairing_tile()` square that stays a cover (*Click to show*) until the
    listener shows it, then the QR code of `SyncShown::link` (`resonate://pair/<code>`, the `qrcode`
    crate, level M, painted as quads in runs of dark modules on `scanned_paper` with `scanned_ink`,
    the one place a colour is not the palette's: a scanner wants black on white); beside it the code,
    covered by `•` until shown, with *Show*/*Hide*, *Copy* and *New code* (armed by the first press,
    `regenerating_the_code`; the second clears `sync-key`, and the binary's sync watcher mints a new code at once), then *Join another
    device's code* (read by `Stored::reads_as_a_code`, the binary's `SyncKey::from_code`, whether or
    not sync runs). `RootView::pairing_shown` falls back to covered with every armed press lowered
    (leaving the category or the pane, or the pointer leaving the body) and after a new code.
  - *Your devices* (keyless): this device first, then online before away, each a raised row with
    its platform's mark (`Icon::Computer` or `Icon::Phone`, `SyncedPlatform`) and *On this network*,
    *Online* or *Away*; one that is away has *Remove* (`Syncing::forget` by `SyncedDevice::id`).
  - *Connection*: a `kit::segmented` of *Resonate's server*, *Your own server* (shows the server
    field; `own_server_asked` holds the choice until an address is given) and *This network only*
    (`sync-nearby-only`, off by default), the chosen one's meaning under it, then the device name.
  - *Songs between devices*: the three `sync-songs` switches.
  Each writes `ResonateApp::synchronised` and the file, which the binary's sync watcher reads again
  within seconds (`sync.md`); a reset never forgets the code. `SyncStanding::Waiting` is the
  service not yet running (just switched on, or another Resonate holds the catalog's sync). The `Syncing` seam on `Stored` (absent without the `sync` feature) answers the rest: a
  playlist's menu offers *Fetch N missing songs from* each online device and *Keep its songs here*;
  the queue heading offers *Continue from* up to two devices whose last queue holds songs, which
  loads them and seeks to the place the other device was at (`Continuing::at`).
- **Discord = two Desktop groups; they write a global before a file.** *Discord* (switch, application
  id) and *What Discord shows* (text, picture, icon, bar, pause). Each control changes
  `ResonateApp::presence`, stores its `Setting`, hands the whole `Presence` to `Present::follow`. Id and
  icon are `Field`s read on enter via `AppId::parse`/`Icon::parse`; unreadable text is a
  `Notice::Trouble` storing nothing; blank clears the key.
- **A boolean is a `kit::switch`, a choice a `kit::segmented`; only a device or palette is still a row.**
  A switch names what it does and what off means, whole row = hit area; a `Choice` is a trough of
  segments. Vocabulary: `kit::section`, `field`, `segmented`, `switch`, `preview`, `dot_swatch`,
  `choice_row` with its `radio`; the pane draws none itself. **`kit::segmented` answers with the trough
  itself; the caller hugs it** (a wrapper round the trough put every segment beside it): watch for
  builders returning a parent their children will not reach.
- **A device reads as what it is and what it takes; its node name is behind the pointer.** Each choice,
  *Follow the system default* included, is a `kit::choice_row`: `kit::radio` held
  `kit::level_with_the_choice` beside a column whose first line is a `kit::choice_name` (`flex_1`, ends
  in an ellipsis; badges `DEFAULT`, `UNPLUGGED`, `IN USE`, the playing track's verdict `TAKES … AS IS`/
  `CONVERTS` follow on the same line). The chosen row has an accent border over an accent wash and the
  filled radio as its only marks. Under the name, two `kit::details` lines divided by faint `·`: how the
  sink is reached, in words (port, the card's profile, whose volume it is), and what it takes, in
  `kit::figure`s (depths/rates via `format::depth` and `SampleRate::kilohertz`, every digit the rate
  holds, so 22.05 kHz is not 22.0). The node name is what the settings file holds, so the row `names`
  itself by it; `SinkInfo::is_hardware` is drawn nowhere.
- **A section holds a subject, not a control** (theme shelf and accent swatches = one *Colour* section).
  A group's hint is written once, in the table; the header's info mark is its only drawing (a gpui
  tooltip is built from an `AnyView`, not a string).
- **Every group ends in a line saying what it does; a choice says what the chosen option does.**
  `Choice::meaning` is required; `RootView::choices` draws the chosen option's meaning under its trough
  as a `note`; `detail` stays the hover for figures such as the filter's taps. A list/button-row group
  (devices, music folders, scan, bindings, About's readbacks) ends in a `note` of its own, always the
  one `note` builder at `text_sm`. *Lossy sources* = *Artificially enhance lossy files*, with the
  `EXPERIMENTAL` badge `Group::is_experimental` puts in a header; its options say in words what they
  take or add, and `LOSSY_ONLY` says that a repaired file is no longer bit-perfect.
- **A palette is shown, not named.** `kit::preview` = a strip of four bands (sidebar, panes, what it
  raises above them, the accent that would be worn) under the palette's name in a card with an accent
  border when worn. An accent is a round `kit::dot_swatch` with a check in `theme::ink_over`'s ink.
  Appearance is the one category with nothing behind it but the window: a choice is worn via
  `theme::wear`, saved through the `Settings` seam, redrawn by `cx.refresh_windows()`; no `Command`.
  Groups in order: *Colour*; *Layout* (text size); *Window buttons* (worn via
  `RootView::show_window_buttons` onto the global, not `theme`); *The volume wheel*
  (`ResonateApp::scroll_volume`, `scroll-volume`); *Mouse navigation* (`mouse-navigation`);
  *Scrollbars*: Always / While scrolling (`ScrollbarMode::AutoHidden`, default) / Never
  (`ResonateApp::scrollbars`); *Sidebar tabs*: `suggestions-tab`, `missing-tab`, `tab-counts` onto
  `ResonateApp::tabs` via `RootView::show_tabs`; *Window state* (`remember-*`); *Live spectrum*.
  `Pane::is_shown` is what a tab being off means: sidebar and `stepped_pane` skip the pane, `set_pane`
  lands on Tracks instead, hiding the front pane moves off it at once.
- **gpui scrolls a region and draws no bar, so `views/scrollbar.rs` does.** `Scrollbars::of` reads the
  setting where a pane is built; `vertical`/`horizontal`/`around` answer a bar over the region's edge,
  or an empty absolute div under `Hidden`. A `horizontal` bar is inset `SHELF_INSET` (24) each side. A
  bar reads the region's own `ScrollHandle` and paints the thumb in a `canvas`. **Nothing a bar knows
  survives a render** (a builder-made `Cell` is new each frame): it lights by weighing
  `Window::mouse_position` against its bounds in paint, which writes those bounds into a cell the same
  frame's listeners read; where on the thumb it was held is taken when the drag *starts* (in `on_drag`'s
  constructor). **`ScrollbarMode::AutoHidden` shows a bar from the offset alone**: paint keeps the last
  offset in element state under `LAST_SCROLLED` (read every paint, since gpui drops state no frame reads)
  and draws the thumb while that offset moved within `SCROLLED_LINGERS` (1.2 s) or the pointer is on the
  bar/thumb held; a paint seeing the offset move spawns one timer refreshing the window when the linger
  is out. **The lyrics pane's bar draws only while the sheet is moved by hand**
  (`Scrollbars::vertical_while`, `LyricsModel::moved_by_hand_lately`, `BAR_LINGERS` 1.5 s; `is_turning`
  keeps frames coming): a gliding sheet's thumb read as a second, busier line.
- **A setting off its default says so; the mark puts it back.** `defaults.rs`: `Standing` = everything
  the pane can put back; `differs` weighs it against `Standing::as_built`; `puts_back` answers the
  restoring commands. A differing section grows an `Icon::Undo` in its header; the footer has *Reset
  <category>* for every category with a key to put back; About's *Reset everything* arms on first press,
  fires on second. Putting back = send the default and take the key *out of the file* via
  `SettingChange::Forget` (a build whose default later changes is followed, not pinned). **Settings are
  written off the render thread; a run is one write**: `RootView::store`/`forget` hand a `SettingChange`
  to `SettingsWriter`, which sends what gathered to a thread of its own (`resonate-settings`) as one
  batch, one in flight, arrivals meanwhile the next; `Settings::apply` takes the whole batch. A failed
  batch is one toast; dropping the writer sends what is gathered and joins the thread. A keyless group
  (folders, *Rescan, refresh and retag*, About's) offers no mark (test-held).
- **A setting is found by typing, not by remembering the tab.** `RootView::finding` = the pane's own
  `Field`, reached by `ctrl-,` or a press, counted by `RootView::editing`, cleared and blurred by escape
  ahead of the contact field in `dismiss_search`. `Narrowing` matches every word against a group's
  title, hint, category and other names (`sink` finds the device, `latency` the buffer, `sacd` DoP).
  With a query the heading reads *Found*, the rail draws match counts, and the body draws matching
  sections from *every* category under a category eyebrow. **Ranked by how they answered**: a group
  answers each word by title, other name, category or hint, in that order of strength, weighed as its
  weakest word; a category leads by its best group, so no category is drawn under two eyebrows
  (`a_setting_named_by_the_words_leads_one_whose_hint_merely_mentions_them`).
- **A right press answers with a menu; `views/menu.rs` is all of how.** `Menu` = position + entries
  (icon, label, optional key, closure); `Menu::at(…).does(…).under(…).apart()` builds,
  `menu::opens_a_menu` attaches to any element: a pane says what a press offers, never how a menu
  draws. It renders as `deferred(anchored()…)` over an occluding scrim taking the outside press; one
  `Option<Menu>` on `RootView`, one arm in escape's chain, one at the head of `dismiss_search`. Not built
  on `tooltip` (`hint.rs`'s source-walking test forbids it). No key context: `reach_row` and
  `play_reached_rows` step/press the menu before reaching for a row. **`on_click` never fires for a
  right press** (gpui 0.2.2 records a pending press only for `MouseButton::Left`), so a menu-opening
  press plays, scopes and seeks nothing. Track row entries: plays, queues, holds for a playlist, reaches
  artist and album (*Go to album* lives only here), file manager, copies. **A row's destination is read
  when the entry is pressed, not when the menu opened**: `Menu::reaches` takes the row's catalog id and
  the press asks `LibraryModel::standing_of` for the track as the catalog holds it now (a menu open
  across a scan that gathered the album away opens the album it was gathered into); a row no scan has
  seen goes where the menu said. `Menu::offers_the_file` is the file group, written once for tracks,
  queue and playlist rows, given the row's `Called`, whose album `RootView::album_named` reads off the
  catalog or, for an unscanned row, the player's tags. *Show in the file manager* = gpui's `reveal_path`
  (portal `OpenURI.OpenDirectory`). **The playback bar's cover, title, artist and album open one menu**
  (`what_plays_menu`); the inspector's title copies whole on right click (`copied_on_a_right_click`).
  A sidebar row, settings control and missing row deliberately carry none.
- **The reached row's menu opens from the keyboard** (`menu`, `shift-f10`: `OpenTheMenu`). A row
  drawing its menu through `menu::opens_a_reachable_menu` with an `AtTheReach` lays a canvas whose
  prepaint keeps its bounds and builder as the `ReachedMenu` global; `open_the_reached_menu` builds it
  only where the global names the reach in force, so a row scrolled away or a moved reach opens nothing
  stale. It opens from the row's left edge at its middle, first entry reached, so the arrows and
  `enter` press any entry
  (`the_menu_key_opens_the_reached_rows_menu_and_the_arrows_and_enter_press_an_entry`).
- **Copies go to the compositor over `ext-data-control`, not gpui.** gpui 0.2.2's Wayland
  `write_to_clipboard` sets the selection under the last *key* press's serial, so a pointer copy left
  KWin's old clipboard. `clipboard::copy` is the one way to copy: own connection, binds
  `ext_data_control_manager_v1` and the seat, sets a source offering the text types, waits
  `ANSWERED_WITHIN` (250 ms) for the round trip; thread `resonate-clipboard` answers each `send` until
  the next copy cancels the source. No data control (GNOME restricts it): gpui's own write. **The UI
  thread never waits; a late answer cannot overwrite the fallback**: `copy_through` races the thread's
  oneshot against an `ANSWERED_WITHIN` timer; one `Claim` (atomic, settled once) decides who acts, the
  fallback written only once the compositor refused or the timer won. **A later copy wins however round
  trips land**: each copy takes a ticket from `Copies` (per `App`), and claim, `set_selection` and round
  trip run under `Issued::handing_over`
  (`a_copy_outrun_by_a_later_one_neither_claims_the_compositor_nor_writes_the_fallback`). Uses
  `wayland-client` + `wayland-protocols` (`staging`), already linked by gpui; no `unsafe`.
- **A scope remembers where it was opened from; its way back names that place.** `Wayback` = pane,
  selection, row the list was left on, place's name, in a `WAYS_BACK` (16) stack; every scoping gesture
  goes through `RootView::opened`; `RootView::here` reads the name as the scope is left. `goes_forward`
  is the bounded forward stack, cleared by a new scope, another category or `show_everything`. With
  *Mouse navigation* on the mouse's back/forward buttons use the same stacks. **A category is a place
  too**: `choose_pane` keeps the page it left in `came_from` whenever pane or selection moved
  (`the_way_back_and_forward_crosses_from_one_category_to_another`). **The page's way back keeps to the
  tab pressed; a link is followed back across.** Each `Wayback` says how it was left (`Leaving::ByALink`
  for `opened`, `Leaving::ByATab` for `choose_pane`); the chevron and Escape (`RootView::step_back`,
  `way_back_to`) take `way_back_within`: the newest page standing under the category in front, or one
  left by a link before any tab press is passed. So album → artist → back returns to the album, but
  album → artist → *Albums* tab → back steps to that album, then to the albums, never to the artist's
  tab; a page passed over stays for the mouse's back button
  (`the_way_back_on_the_page_keeps_to_the_tab_pressed_and_follows_a_link_across`). Escape on an unscoped
  page steps back only where that way back leads to a scope (`goes_back_to_a_scope`). The way back is
  `kit::way_back` at the page's top left (*‹ Tracks*); with nothing behind it, it reads the category the
  page stands under and clears the scope. **A list is left at a row and how far into it**: `LeftAt`
  reads the scroll offset against `row_height_of` (grid row, tall artist row, plain row) as row +
  fraction, so a list lands on its pixel, and one whose rows grew with the text size on the same row
  (`a_list_whose_rows_grew_lands_on_the_row_it_was_left_at_and_as_far_into_it`). Landing waits on
  purpose (`land_where_it_was_left` holds until the pane has that many rows, each via its own scroll
  handle); `landing_on` is set only for a `Pane::lands_where_it_was_left` and `set_pane` clears it.
- **A queued row's name is weighed against the row, not only its id.** The engine mints ids from
  `u64::MAX` down afresh per restore, unscanned playlist row and doubled row, so two files can carry one
  id a load apart. `LibraryModel::track_of` keeps location and span beside the name it read and re-reads
  where they disagree. `album_title` answers for any album (scoped, listed, or read by id into a
  bounded `read_albums` the next listing load discards). **The queue pane's per-row weighing is off the
  render thread**: `queue_heading`'s total is measured once per queue and library revision in the
  background (`queued_rows`, `QueueMeasure`), reading *at least* it where a row has no scanned length
  (`QueueLength::unmeasured`); a sort chip keys every row in the background too (`ordered_rows`),
  sending the order only if the queue is still the revision it keyed.
- **A row is keyed by what it holds, not where it stands.** gpui keeps tooltip, hover and drag in
  element state under the id path: an index-keyed row handed row N's tooltip to whatever an edit moved
  into N. Keys: queue row = queue id; track row = `TrackId`; artist row = `ArtistId`; missing row's want
  mark = release track or found recording; playlist entry (may hold one cut twice) =
  `listing::keyed_by` over cut and place. **A row gesture reads the row the pane drew, not the list's
  row at that index**: a search-narrowed playlist's *Take out of the playlist* drops
  `Span::one(entry.position)` (the row's place in the list); enter on a reached row inside an album
  goes through `LibraryModel::played_from` (display row to the track it draws; nothing for a disc
  heading or missing row). Tracks-pane rows draw `Plays::AsTheListingIsDrawn`; favourites' and a
  suggestion's `Plays::TheseRows`.
- **Every control is keyboard-reachable through gpui's own ring.** `views/focus.rs` holds one
  `FocusHandle` per control id, forgetting those a frame stopped drawing; ring = `Window::focus_next`/
  `focus_prev` over tab stops gpui inserts in paint order. A control carries `.track_focus`,
  `.tab_stop(true)` and `key_context(CONTROL_CONTEXT)` (`RootView::ringed`): `space`/`enter` bound under
  it fire the control's own `on_action`; escape lets go; every window binding that carried `!Search`
  carries `!Search && !Control`; `tab`/`shift-tab` bind with no predicate. Focus is drawn as hover is.
  - **A press by key is the action's alone.** gpui 0.2.2 also turns the key-up on a focused element
    with click listeners into a `ClickEvent::Keyboard`, so every `on_click` beside a `PressControl`
    action (`ringed`, `in_the_ring_at`, `option_in_the_ring`, the device and root rows) answers only
    `focus::by_the_pointer`; `space`/`enter` would otherwise press twice (a switch flips back, Next
    skips two). The driven tests send the key-up as well (`enter_pressed_and_released_on_next_skips_one_song_not_two`).
  - **A tab stop is the handle's, not the element's.** gpui applies an element's `.tab_stop(true)` only
    to a handle it made itself, so `Controls::at` and `Field::new` make their handles tab stops.
  - `RootView::content` opens each pane frame with `Controls::opening`, keeping the handles the frame
    before drew, so a control a `uniform_list` row draws keeps its caret. Settings lays its own through
    `in_the_ring`; every other pane through `RootView::in_the_pane_ring`, which names the handle by the
    control's own element id, presses it through the one closure a click and `space`/`enter` share, and
    takes no caret on a click. A row's hidden controls are `RootView::row_controls` (a handle that is no
    tab stop, `Controls::holding`), shown by `in_focus` while one holds the caret
    (`tab_reaches_a_headings_play_and_a_rows_star_and_enter_presses_them`). A mark opening a card or menu
    where pressed goes through `in_the_pane_ring_at` (a canvas keeps its bottom-left corner;
    `a_mark_opening_a_menu_where_pressed_opens_it_from_the_keyboard_too`); the ring wrappers `_at`
    hand the caret back to the window as they open it, so the menu's arrows and `enter` answer. The
    playback bar's sleep timer is `in_the_standing_ring_at` (`tab_reaches_the_sleep_timer_and_enter_opens_its_menu_and_sets_a_timer`).
    A lyric line is a tab stop through `in_the_pane_ring_for_keys_at` (`ringed_for_keys`: the
    `PressControl` action alone, so the line's own pointer click, guarded by `by_the_pointer`, stays the
    seek): `enter` opens the line's menu, *Copy this line*, *Copy the whole sheet* and, on a timed
    line, *Play from this line* (`tab_reaches_a_lyric_line_and_enter_opens_a_menu_to_copy_it_or_play_from_it`).
    **A click on a line takes no caret**, as every pane ring's press does (`prevent_default` on the
    left press): a focused line is drawn at full opacity, so a line clicked to seek stayed lit after it
    was sung (`a_lyric_line_clicked_seeks_without_keeping_the_caret_that_would_hold_it_lit`).
    **A line reached by tab is brought into view**: each drawn line carries a canvas
    (`brings_into_view_when_reached`) asking its handle in prepaint; `LyricsModel::reached` scrolls
    the sheet just far enough (`moved_to_show`, clear of the `dissolve` edges, snapped to whole
    pixels) once per reach (`left` lets go) and `led_by_hand`, so a synced sheet's glide does not
    carry the line away for `HANDS_OFF`
    (`tabbing_down_an_unsynced_sheet_keeps_the_line_reached_in_view`).
  - **The seek and volume rails are tab stops of their own context**: `slider::rail` under `Control
    Rail`; `app::answering_on_a_rail` binds `right`/`up` to `RailOn` and `left`/`down` to `RailBack`,
    stepping a seek by `seek_step`, a volume by `VOLUME_STEP`
    (`a_rail_holding_the_caret_moves_with_the_arrows`).
  - `standing_controls` (transport buttons, mute mark, sidebar panes) are never pruned
    (`in_the_standing_ring`); a click on one takes no caret (`prevent_default`), so choosing a pane with
    the pointer leaves the arrows reaching its rows. `tab` from the window moves past the search field,
    `shift-tab` returns to it
    (`tab_reaches_the_transport_buttons_and_the_sidebar_and_enter_presses_them`).
- **What is in this build, and where it keeps things, is a category, not a window.** `WindowKind` is
  `Main` alone. About reports the version, the two faces `fonts.rs` settled on, the sink count, whether
  a reference started; names the settings file and the catalog (`Places` on `run` beside the `Settings`
  seam in `Stored`). Readbacks, not settings.
- **DoP is a hardware claim; the pane says so.** `Command::SetDop`, `OutputSettings::dop`,
  `Setting::Dop`; off by default; the hint repeats `audio.md`: only the DAC's own detector knows, and a
  DAC not decoding it plays the markers as full-scale white noise.
- **A value off a choice's table is said, never left unlit.** `RootView::choices` takes the value in
  force: a segment lights where it equals one of `Choice::ALL`; where none does, `said_under` writes
  *… is in force, which is none of these.* via `Choice::in_force` (the figure itself where the key reads
  any number: `buffer-ms`, `listen-for`, `bluetooth-lead-ms`, `bluetooth-awake-s`, the ReplayGain trims,
  `history-kept`). Those choices are newtypes over what the key holds (`BufferDepth(Duration)`,
  `ClipLength(Duration)`, `PreAmp(Trim)`), so the pane cannot turn an off-table value into `None`. A
  device named in the file but absent from the graph marks no row; the group says so.
- **A pass that rewrites files is armed before it runs; the preview arms it.** *Organising* and
  the tags part of *Rescan, refresh and retag*: *Preview* runs the pass with `Pass::Preview`; *Apply* is greyed until a preview landed,
  then takes two presses (the second under a note saying what the first armed); *Stop* is drawn only
  while it runs. *Put the last run back* (`walks_back`, `tags_walk_back`) is armed the same way.
  `disarm` (leaving the pane or category) and the pointer leaving the settings body (a transition via
  `on_hover`) run `lower_every_armed_press` (these four, the reset, the vault's keep, the curve discard,
  history aging). Passes run through `Work`, read by `LibraryModel::is_busy`: scan, forget, organise,
  tag write cannot overlap, each greying the others' controls (`Library::retag`, `scan` and `organise`
  share one `Walk` guard).
- **A preview is taken down once the catalog it read has moved.** `Planned` (held by *Organising*,
  *Tagging*, the vault's *Import*): `Not`, `Shown` with the `resonate_library::CatalogStamp` taken as the
  preview started, or `Outdated`. Every reload's `landed` weighs a shown preview against
  `Library::plans_stamp`; stamp moved = drop rows and counts, grey *Apply*, say the catalog moved. The
  stamp is a counter on the writer connection (temp triggers over the columns a plan reads,
  `PLANNED_FROM` in `db.rs`) beside `PRAGMA data_version` for another process: a lookup landing a
  release or a scan reading a file anew takes the preview down; a counted play or favourite does not.
  **The tags part draws the plan, not a count of it**: `listed` = first `WRITES_SHOWN` (12) of
  `Retagging::writes`, one raised row per file over the fields to be written, *and N more*; the count
  stays the pass's own so note and rows cannot disagree.
- **Everything that brings the catalog up to date is one Library group, in three parts.** *Rescan,
  refresh and retag* (`Group::Updating`, `updating_group`): *The files* (*Rescan folders*, *Read every
  file again*, *Stop* while a scan runs), *What Online knows* (*Look up*, *Refresh all*, *Look for
  missing covers*, *Stop* while a lookup runs) and *The tags in the files* (the armed *Preview*/*Apply*
  pass below). *Read every file again* (`Reading::Everything` beside `Prompted` on
  `LibraryModel::start_scan`) re-probes every file whatever size and mtime say, each row keeping id,
  plays, favourite; *Look for missing covers* is `Library::ask_again_for_covers` + a lookup; *Refresh
  all* is `enrich(true)`. The files part greys with `is_busy`, the lookups with `is_busy`,
  `is_enriching` and `!can_enrich`.
- **Library roots are edited through the desktop's file picker, or typed.** `App::prompt_for_paths`
  (`directories: true`) is the XDG portal; a machine without one says so and points at
  `RootView::typed_root` (*Or type a folder, then press enter*). `root_typed` reads a leading `~` as
  `$HOME`, refuses a non-directory in the pane's own notice, hands a directory to `add_roots`. Adding
  scans just the folders named; *Rescan* walks registered roots; dropping one goes through
  `Library::remove_root`, forgetting every track from it. Roots ride in the one `Loaded` snapshot and
  live in the library database, not `config.toml`: the one setting outside the `Settings` seam.
- **The inspector renders the engine's `StreamDigest`, never a second read of the file** (`audio.md`).
  Order: signal path as three `stage` cards (*Source*: codec in the lossless colour over depth, rate,
  channels, container; *Processing*; *Output*: mode in its colour over what was negotiated and the
  sink), detail cards, bitrate graph, tags. Follows the *playing* file only. The live profile restarts
  on every rebind (`StreamDigest::profiled_from`: a `ProfileBuilder`'s windows are unresumable
  mid-span); the digest is rebuilt once per closed window, and the decoder runs ahead of the sink, so
  the graph leads what is heard. A line, not bars: `plot::stroke` over a `plot::wash` from the accent to
  nothing. Condensed at the digest's rate, not the frame rate: `PlayerModel::condensed` holds the reduced
  series and drops it when the poll swaps the digest for a different `Arc` (`StreamProfile::series`
  runs to `MAX_WINDOWS`, a whole day; reducing to `GRAPH_COLUMNS` sixty times a second is a walk and
  allocation a frame).
- **The visualiser draws what the tap says is being heard; its arithmetic has no gpui.**
  `Pane::Visualiser` under `Section::Playing` (same six edits), following the playing track alone:
  nothing playing = `kit::empty` under `Icon::Visualiser`; a DoP stream = `kit::empty` saying it carries
  markers, not samples. Heading = lyrics pane's with a `kit::figure` reading back the analysis
  (*4096-point · 48 kHz*) and a `kit::segmented` *Spectrum*/*Scope*/*Stereo* (`Showing`) living on the
  entity for the run. `spectrum.rs`: hand-written radix-2 real transform under a periodic Hann window,
  scaled so a full-scale sine on a bin reads 0 dBFS; bands (an octave split by `spectrum-bands`, sixths by default) from the equaliser's
  `RESPONSE_FROM_HZ` to what the rate carries (`spectrum::top_of`: Nyquist, never under
  `RESPONSE_TO_HZ`), placed by `spectrum::across`, labelled per decade (`spectrum::marked`). A band = its
  loudest bin tilted up about 1 kHz (`spectrum-tilt`, 3 dB an octave by default: pink noise stands level), drawn between
  the floor (−78 dB) and 0 over faint lines every 12 dB (no figures: a tilted reading is dBFS only at
  the pivot). A bar rises at once and falls at its rate (40 dB/s as built); its peak is held
  `PEAK_HELD_FOR` (700 ms) before following at half that; all wall-clock. **Tilt, floor, bands an octave
  and fall rate are the listener's**: `resonate_core::Spectral` (`spectrum-tilt`, `spectrum-floor`,
  `spectrum-bands`, `spectrum-falls`) rides `Stored` onto `ResonateApp::spectral` (Appearance's *Live
  spectrum* group; the pane's hints are written from it, `reads_hint`, `spectrum_hint`); the visualiser rebuilds its `Spectrum` where the reading or rate moved (the stereo
  meters keep `Falling::METERED`). Window sized by rate, about 85 ms (`ANALYSED_FOR`). Scope = 20 ms of
  left over right from the first rising zero crossing, of their mid or their side where it carries more
  energy (`spectrum::louder_of`), armed only once the trace has fallen a tenth of the window's loudest
  below zero (`ARMED_UNDER_THE_LOUDEST`).
  - **Stereo** (`stereo.rs`, gpui-free): each channel's mean square in dBFS into a `Bar`, its true peak
    (*dBTP*: a `TruePeakMeter` per channel fed only the frames that crossed the window's centre since the
    last frame, `Caught`, so cost follows the rate, not the display) into another, and the channels'
    Pearson correlation settled over `CORRELATION_SETTLES_OVER`. The canvas draws a goniometer
    (`sides_and_mids`, at most `DOTS_AT_MOST` dots), two level meters and the correlation on a −1..+1
    track (failure colour below 0); held levels and correlation are figures explained behind the pointer.
  - **The pane follows the display's clock while it has sound to draw**: a zero-size canvas asks
    `Window::request_animation_frame` whenever the transport plays and the engine hands the pane a tap
    (cached regions made it affordable); paused or nothing tapped, it asks for nothing.
    `RootView::new` calls `PlayerModel::listen_in(true)` so the plot opens on what is already heard
    (cost in `audio.md`).
- **The analysis pane draws the whole playing track and says whether it is what it claims.**
  `Pane::Analysis`: fourth pane under `Section::Playing`, `Icon::Analysis`. Heading = visualiser's,
  reading back codec, declared depth and rate, or how far the decode has got. Then: verdict in its colour
  over one sentence per finding (`Finding::told`); waveform (lane per channel, min-max polygon washed in
  the accent with RMS solid inside, heard part shaded, playhead a press seeks from); spectrogram (painted
  raster stretched to the card, cutoff a line in the verdict's colour); average spectrum; levels and
  source `fields` cards; a recognition card, led to the top where the audio is not the song the file
  names, carrying *Take this name*. `analysis_plot.rs` is the arithmetic, no gpui. Online's
  *Recognition* group is the `acoustid-key` field, read from the next start. `analysis.md` has the model.
- **Listen is a sheet over whatever pane is in front, not a pane.** It names a song that need not be in
  the library: `ctrl-l` (in `answering_anywhere`) and the header ear open it via
  `RootView::open_the_listener`, recording at once; escape or a press outside closes it and stops a
  recording in flight, dropping the task that would ask about the clip. `listening.rs` = model,
  `views/listen.rs` = drawing. `ListenModel` holds the `Listens` the binary handed in and walks one
  `Stage`: `Recording`, `Asking`, then `Found`, `Unknown`, `Silent`, `CaptureFailed`, `Unreached`,
  `Refused`, `Offline`, `NoService`. **A miss is listened to again**: a clip no service named, or one
  nothing reached, is recorded again up to `TRIES_ON_A_MISS` (3); a service out of reach or refusing is
  said at once. **Keep listening follows the music**: a switch (`keep_listening`, kept for the run)
  records another clip `FOLLOWED_EVERY` (20 s) after every named song and last miss (`Following`); the
  same title and artist (folded) is kept quietly, a new one is told to the desktop and heads the list, a
  miss while following changes nothing. `then` is the whole decision, pure
  (`a_miss_or_a_silent_clip_is_listened_to_again_until_the_tries_run_out`,
  `following_tells_a_new_song_and_keeps_quiet_about_the_same_one_or_a_miss`); `stage_after` picks the
  landing for an error (`a_service_that_refused_the_clip_is_not_one_that_could_not_be_reached`).
  Recording and asking run on the background executor. `listen` takes whether Online is on *now*
  (`RootView::listen_now`): off = `Offline`, nothing recorded; on with no recogniser = `NoService`, saying
  they come at the next start (`online_switched_on_in_this_run_is_not_said_to_be_off`). The recording
  note names the source in the stage, not the one chosen; source segments and microphone chips take no
  press while `is_listening` and `listen_from` refuses one
  (`a_source_pressed_while_recording_is_refused_and_the_recording_names_its_own`). A found song is a
  card: cover at `theme::listen_cover()`, title, artist, album, year, naming service, *Listen again*,
  *Open* where the service gave a link, *Find it* (`search_instead` + `choose_pane(Pane::Tracks)`), laid
  in `sheet_actions` (`kit::actions` is a heading's row pushed right). The last `HEARD_KEPT` (8) songs
  stay listed for the run. *Desktop*/*Microphone* = a `kit::segmented` over chips of the microphones
  PipeWire offers (listed each time the sheet opens), written back as `listen-from`; Online's *Listening*
  group writes the same key and `listen-for`; *Recognition* has the AudD token beside the AcoustID key
  (read at the next start); *ListenBrainz* is the `listenbrainz-token` field, not read at next start (the
  binary's submitter follows the settings file). With a token typed, *Send earlier plays* is armed by
  the first press (`telling_the_earlier_plays`) and asks `Library::tell_earlier_listens` on the second
  (`the_earlier_plays_are_asked_for_by_the_second_press_alone`). **A token given is asked about at
  once**: where Online is on and the build handed `Lookups::scrobblers`, `check_the_listenbrainz_token`
  asks `token_held` on bg and toasts whose token it is or that the service does not know it.
- **Play next and Add to queue are row actions; Shuffle is the tracks heading's second play action.**
  Each track row, the playlists index row and each opened-playlist row carry the queue pair (on a
  playlist row they take the whole reach the row is in). The + on a queue row and an opened-playlist row
  opens the playlist picker; index and opened-playlist headings use + to enter track-browsing mode.
  Albums and artists panes, the queue pane, the album page, artist page and opened suggestion carry
  neither. Named once in `menu::PLAY_NEXT`/`menu::ADD_TO_QUEUE`: *Play next* = `Placement::Next`; *Add to
  queue* = `Placement::Queued`. The queue pane marks a row waiting to play next with the *queue next*
  icon where its number would be (`Queued::next`); its heading counts them.
- **The queue is drawn in the engine's four parts, each under a heading.** Published list =
  `order[..after] ++ next ++ order[after..]`: `QueueParts::of` reads length, `PlayerState::queue_position`
  and `Queued::next` into runs of `Part::Heard`, `Playing`, `Next`, `Rest` (*HISTORY*, *NOW PLAYING*,
  *PLAYING NEXT*, *CONTINUE PLAYING*, the last naming the playlist where `Library::playing_playlist`
  still badges one). A queued row being heard is `Playing`, not `Next`. A heading is one more
  `uniform_list` row; a one-part queue draws none. Heard rows draw at `HEARD_FADED` (0.55). Gestures
  still speak in queue rows; `QueueParts::line_of`/`line` are the one mapping to the list's lines
  (`every_row_is_shown_at_the_line_that_draws_it`).
- **Every major listing is ordered from a chip row and a header.** Tracks, albums, artists each hold an
  order and a reading on `LibraryModel::sorting`, kept for the run. Two writers: a sort icon opening the
  ORDER and READS chips, and the pressable column header (a press names the order; a second on the one
  in force reverses it; the column in force wears the accent and a chevron). `views/sorting.rs`:
  `Ordering` is the trait the five order enums implement, `sorting::order_row` the one chip-row builder;
  `listing::columns` takes a `Sorted` descriptor (columns offered, which is in force, a `fn` pointer for
  the press), so tracks pane, queue and opened playlist draw one header. A right press on a header
  offers every order.
- **The queue is ordered as a gesture, not a standing order**: its heading's sort icon sends one
  `Command::Order` (a permutation over the rows the window draws); the engine rewrites the play order in
  one pass with the cursor re-seated, so a mid-track sort reopens no stream. *Clear* sends one
  `Command::Remove` over the whole span. The reach lives for the run, escape clears it.
- **Undo and Redo are drawn in the playlists headings only** (`RootView::undo_edit`/`redo_edit`; Redo only
  while a step is there; each says how many steps stand behind the one it offers, `Undoable::behind`).
  Rows put in a playlist from the tracks or queue pane have no control to press, so the notice names
  `ctrl-z`. **A narrowed playlist says what a gesture reaches** (`NARROWED_HINT`): Play, Play next, Add
  to queue, Drop shown take the rows shown; Sort, Tidy, Export take the playlist whole. Tidy removes
  missing and repeated rows together in one undoable edit. The sidebar's count is the narrowed one.
- **A pane hands its rows out by reference count, not cloned per frame.** An opened playlist's rows are
  an `Arc<[PlaylistEntry]>` shared by model, pane, heading, list closures and every visible row; the
  reach is read off it once a frame as a `Reaching` (span + the cuts it holds); `Held` carries an
  `Arc<[Cut]>`. `LibraryModel` holds albums, artists, tracks as `Arc<[_]>`. A whole list is walked only
  where a press asks (*Duplicate*, the index row's +, *Save as a playlist*, the tracks heading's *Add to
  playlist*, *Play next*, *Add to queue*, *Play*), inside the listener.
- **The playing row is indexed, not searched for, and resolved once per pane.**
  `RootView::queued_row` indexes the published queue by `PlayerState::queue_position` and weighs the id
  found against `state.current`. One `Playing` carries all the playback bar draws (title, artist,
  album, codec, a `Cover` of album id and picture-source file).
- **A playlist edit re-reads the playlists and nothing else; so do a scan and a counted play.**
  `LibraryModel::reload_playlists` takes the `ThePlaylists` read; `reload` re-reads the opened playlist
  beside the listing (a scan runs it every `POLLS_PER_RELOAD` (20) polls and at the end; a counted play
  once), so `plays:` and `played:` queries move under the window as `added:` does. A rename re-reads the
  entries beside the whole listing: counts and total length are one grouped pass over the entries.
- **A second process's edit is seen once its writes settle.** `watch_for_writes_elsewhere` reads
  `Library::written_elsewhere` (the writer connection's `PRAGMA data_version`, a `WrittenElsewhere`
  that is only compared) on a background thread every `WATCHED_EVERY` (2 s) and reloads once the stamp
  moved since the last reload *and* read the same the tick before, so a scan committing batch after
  batch costs one reload when it pauses. This process's own writes never move the stamp; a busy writer
  answers nothing and the tick is passed over.
- **`Raise` activates the window; `Quit` closes it.** The binary's host answers `can_raise` with whether
  it holds a raise channel (the window's does; windowless `resonate play` and the bus's default `Host`
  answer false); `Raise` sends on it, `raise_when_asked` drains it on the gpui foreground and calls
  `activate_window` on every window (a compositor may still refuse without an activation token). `Quit`
  sends on the channel `resonate play` uses, drained by the gpui foreground on the frame timer.
