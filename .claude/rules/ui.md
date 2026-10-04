---
paths:
  - "crates/resonate-ui/**/*.rs"
---

# The window

`resonate-ui` is GPUI on Wayland. It depends on the engine, the library, the lyrics and
`resonate-listen`, and on neither `resonate-codec` nor any image decoding — it names `image` only for
the frame gpui's `RenderImage` is built of; a `Reference` reaches it only as the trait object the binary
hands `run` inside `Lookups`, so it never names the online crate either.

## Chrome and input

- **The window is its own titlebar.** `WindowOptions` asks for `WindowDecorations::Client`, and the
  header carrying the Resonate wordmark and the search field draws the minimise, maximise and close
  controls beside them, drags the window and opens the compositor's window menu. The header is
  `theme::header_height()` tall and three columns: the wordmark and the window controls each take the
  sidebar's width less its padding, so the search field between them centres on the window rather than
  on what the wordmark leaves, growing no wider than `theme::search_width()`. The Listen button stands
  against the field's right edge, inside the middle column, so the two read as one group. A window
  control is `theme::WINDOW_CONTROL` (31) square — odd, because its mark is odd, so the mark centres on a
  whole pixel rather than rounding low — and the row is pulled right by `mark_inset`, the room between a
  control's edge and its mark, so the close mark stands in the wordmark square's gutter. The word
  *Resonate* is dropped `WORDMARK_CAPITALS_CENTRED_BY` below its line box, centring its capitals on the
  accent square rather than on the line box, whose descender space leaves them high. An empty, unfocused
  field draws `ctrl-f` in the mono face at its end, the one key taking focus deliberately being
  otherwise nowhere on screen. `views/chrome.rs` owns the frame around it: a `theme::RESIZE_BORDER`
  gutter outside the painted window carrying the resize edges and the shadow, and rounded corners on
  whichever edges are not tiled. **The shadow is cast by a rim, not the window.** gpui shades a shadow
  over its whole box, three blurs past it, so a shadow on the window ran its blur arithmetic over every
  pixel every frame for a fringe a few pixels wide — a quarter of what a tracks-pane frame cost the GPU.
  `shadow_rim` casts the same `BoxShadow` from four strips under the content along its edges, each as
  deep as the blur reaches, top and bottom carrying the corners' rounding; a blur being linear, outside
  the window the four sum to the whole box's shadow, and a pixel comparison found no difference past 2 in
  255. **The gutter repaints only where the resize edge under the pointer changes**: the cursor a
  `resize_cursor` sets is read off the pointer in paint, where `frame` used to `refresh` the window —
  every cache dropped — on every pointer move over it. **gpui clips a child to its parent's rectangle,
  not its rounding**, so the header and playback bar, whose `surface` fill reaches the window's corners,
  painted a square past the rounded border. `chrome::rounded` is the one reading of which corners are
  round and `rounded_within_the_frame` the same corners less `FRAME_BORDER`, taken by the header on its
  top corners and the playback bar on its bottom ones, so the fill nests inside the border. **An edge
  held to the screen is not a resize edge.** A tiled edge loses its gutter, the content running to it,
  and `grabbed_edge` reading the outer `RESIZE_BORDER` of every edge turned a press on the bottom strip of
  a maximised window's playback bar into a compositor resize grab that ate the click. It weighs only the
  edges `held_edges` leaves free — none at all while maximised or fullscreen, whatever tiling the
  compositor reported — Zed's frame's rule too, held by `chrome.rs`'s tests. A compositor may refuse the
  request, gpui then reporting `Decorations::Server`, so the controls and drag handlers are rendered only
  under `Decorations::Client` — a window keeping a server titlebar must not grow a second set of buttons.
  **Which of minimise and maximise are drawn is two readings weighed together**: `Window::window_controls`
  (what the compositor offers) and `ResonateApp::window_buttons`, a `WindowButtons` the `minimise-button`
  and `maximise-button` keys fill and the Appearance category's *Window buttons* switches write.
  `RootView::window_controls` draws a button only where both say yes, so a setting can remove one and
  never conjure one a compositor withheld. The switch writes the global and notifies the root view, so
  the header redraws next frame, not next run. Close is not a setting — the one way out a pointer has —
  and a double press on the bar still zooms whether or not maximise is drawn, `chrome::titlebar` never
  asking it. gpui forces every edge tiled when maximised or fullscreen, which drops the gutter, rounding
  and shadow there. `window_min_size` is where the header's own children stop clipping rather than a size
  the panes were designed around, and a compositor may ignore it. **None of the four marks is a glyph;
  all four are icons.** `Icon::WindowMinimise`, `WindowMaximise`, `WindowRestore` and `WindowClose` are
  SVGs whose strokes run to the viewBox's edge where `Icon::Close` is inset, so each renders at the
  square's own edge, their stroke being the 24-unit box's share of one device pixel at the drawn size, so
  bar and square are as crisp as the bordered `div`s they replaced while lighting on hover through
  `icons::lit_on_hover`. All four are `theme::WINDOW_MARK` less its overlap across, putting the three at
  one size on one centre line. A glyph could not: no box is in the primary UI face, so maximise always
  had to be drawn, and − and × centre on the math axis rather than the box, leaving the × half a pixel off
  the square beside it at half its width.
- **Closing removes the window and quitting follows, never the other way round.** The close control,
  `ctrl-q` and the bus's `Quit` all call `Window::remove_window`, and `app.rs` quits once the last window
  closed. `App::quit` under a live window cleared gpui's window map while the compositor still held the
  surface, so the pointer leaving the just-pressed close button arrived at a window no longer there and
  gpui logged *window not found* twice every exit. Removing the window first drops the platform window
  and its callbacks, and Wayland's client stops the loop when its last window goes.
- **The window names itself to the compositor twice.** `WindowOptions::app_id` carries `APP_ID`, what
  `xdg_toplevel.set_app_id` sends and a compositor matches against `resonate.desktop` for the taskbar
  icon and grouping; the entry's `StartupWMClass` is held to the same string by a test in `app.rs`. The
  caption is a second call, gpui's Wayland backend never reading `WindowParams::titlebar` —
  `TitlebarOptions::title` names only the header this window paints — so `Window::set_window_title` on
  open is what names the taskbar and window-switcher entry.
- **The header is a titlebar, so everything clickable in it stops the press.** `chrome::titlebar` puts
  `start_window_move` on the whole bar, and a mouse-down reaching it is a drag, not a click — why the
  window controls and the search field carry `on_mouse_down(Left, stop_propagation)`. A header control
  without one is unclickable, not merely unstyled.
- **The search field is a real text input, not a keystroke accumulator.** `views/field.rs` owns the
  `Field` entity and `edit.rs` the `Edit` behind it — content, caret and selection, with no gpui, so the
  caret, word motions and grapheme steps are tested without a window. The view half is a custom
  `Element`: it shapes the line, paints selection and caret, and registers an `ElementInputHandler`,
  giving IME preediting, a `bounds_for_range` the candidate window is placed against, and a caret
  mapping to and from a pointer position. The line scrolls under the field to keep the caret in view
  rather than truncating, and a selection drag is followed from a `Window::on_mouse_event` registered in
  `paint`, the element's own `on_mouse_move` firing only while the pointer is over it — the reason
  `views/slider.rs` carries a drag surface.
- **TIDAL is signed in to from its group, never by pasting a token from elsewhere.** The *A TIDAL
  account* group holds a `SigningIn` on `RootView` — `TidalSigning::Idle`, `Asking` or `Waiting`
  with the `Authorizing` it shows, a stop flag and the task — and `sign_in_to_tidal` runs the
  `SignsIn` seam's two calls on the background executor, the code drawn as a `kit::figure` beside
  *Open the page* (`cx.open_url`) and *Stop*, which sets the flag the waiting call reads. The token
  it is handed is held in the refresh-token field, the global and the settings file at once; a
  refusal is a toast naming which (`providers.md` has the seam). The group's hifi-api address is
  an optional custom-server override: blank selects the hosted service from the next start, and
  submitting a blank field says so.
- **A secret is drawn as marks.** `Field::masked` — the Subsonic password, the TIDAL client secret and
  refresh token and the AcoustID, AudD and ListenBrainz keys — shapes one `•` a letter in place of the text, and `Shown` maps every content
  offset the caret, selection, marked range and IME bounds hold to the drawn line and a pointer's
  back, so editing is as a plain field's; a masked field copies and cuts nothing to the clipboard
  (`a_masked_field_draws_a_mark_a_letter_and_maps_every_offset_both_ways`).
- **The caret blinks as the desktop says.** `CaretBlink` is a shared half-period on `Stored` and
  `ResonateApp` — `as_built` half a second, `steady` holding it lit — and every field's blink task reads
  it afresh each tick, so what arrives after the window opened takes effect on the next, and a caret held
  still looks again a second later. The binary's `caret_as_the_desktop_blinks` asks on a
  `resonate-caret` thread, so the window never waits: `resonate_mpris::caret_blinking` reads the Settings
  portal's `ReadOne` under a one-second deadline — `org.gnome.desktop.interface`'s `cursor-blink` and
  `cursor-blink-time`, which KDE's portal answers too, then `org.kde.kdeglobals.KDE`'s
  `CursorBlinkRate` — and a cycle of nothing, or blinking off, is a steady caret; a desktop answering
  neither leaves the half second. The cycle is halved, a desktop naming dark and lit together.
  `the_caret_blinks_as_the_desktop_says_and_holds_still_where_it_says_not_to` drives it.
- **The search field's undo step is a span of edits, not a keystroke.** `edit.rs` keeps a bounded stack
  of content-and-selection snapshots and coalesces consecutive edits carrying the same `edit::Span` —
  `Typing`, `Removing` or `Composing`, distinct from `resonate-core::Span` — *and* resuming from the
  selection the last left, so a typed word is one step, a run of backspaces one, and an input method
  revising a preedit one however often it revises. A paste, a cut and a clear are `Span::Discrete` and
  never join what came before, which makes `ctrl-a` over the lot, and escape, recoverable. Whitespace
  closes a typing run, so undo walks back a word at a time. **An edit that changes nothing is no step**:
  `Edit::edited` weighs what the range holds against what would replace it, and a backspace at the
  start, a delete at the end or a paste of what is selected only moves the caret, keeping no snapshot
  and leaving what was undone to redo; `set_content` with the text already there does the same. The history is `Edit`'s, not the view's, so
  it is tested without a window. It holds 128 steps (`UNDO_DEPTH`) and lives only as long as the run,
  and escape clears the field and blurs it in one stroke, so what it threw away is out of reach until a
  click puts the caret back: the three keys are bound under `SEARCH_CONTEXT` and a blurred field never
  sees them. Outside the field the same three walk a playlist edit, so what they reach depends on where
  the caret is.
- **The window names itself, and a negated binding is dead without it.** The root div carries
  `key_context(WINDOW_CONTEXT)` beside its `track_focus`, so the context stack is `["Resonate"]` at rest
  and `["Resonate", "Search"]` with the caret in a field. Not decoration: gpui's
  `KeyBindingContextPredicate::depth_of` returns `false` for an *empty* slice before reaching the `Not`
  arm, so a window naming nothing disables every `!Search` binding — transport, reach, undo and redo
  keys alike — while the field's own bindings work, the field being the one element that named itself.
  The asymmetry is the tell. `app.rs`'s tests pin both stacks against the two predicates, one asserting
  gpui still refuses the empty slice, so a toolchain changing that answer says so.
- **An opened album or artist stands under its category, and pressing the category again leaves it.**
  The page is `Pane::Tracks` narrowed to a `Selection`, but a listener who opened it from the albums grid
  is *in Albums*, so `RootView::in_front` — `in_front_of`, arithmetic over pane and selection — is what
  the sidebar marks: Albums for an album, Artists for an artist, the pane itself otherwise. A sidebar
  press is `RootView::choose_pane`, and `landing` is the whole decision, tested without a window: the row
  already in front goes back to the whole of its category, clearing the scope or closing an opened
  playlist; a category whose page still stands behind another pane goes back to that page, as a tab keeps
  its place; *Tracks* always lists every track, a scope never belonging to it; anything else only changes
  pane. Before, the sidebar marked *Tracks* while an album was open, pressing *Albums* left the scope to
  surface later under *Tracks*, and pressing *Playlists* inside a playlist did nothing.
- **`ctrl-tab` walks the sidebar, and the browse panes answer the reach keys.** `NextPane` and
  `PreviousPane` step `Pane::BROWSE` from the pane `in_front` names, through `choose_pane`, and wrap; a
  pane the sidebar does not list steps onto the first it does, `stepped_pane` being that arithmetic,
  tested without a window like `Step::landing`. `Shift::Listing` names a browse listing by its `Listed`
  — the tracks and artists panes first, each growing a `UniformListScrollHandle` so `show_row`'s centred
  scroll works there — and a reached row wears the queue row's accent edge. What a listing refuses is
  what it has no order of its own to change: `Shift::is_edited` is the reading, so `delete`, `alt-up` and
  `alt-down` are dead there while the reach, the page keys and `enter` are not — `enter` plays a track or
  opens an artist, as a click does. `ctrl-shift-left` and `ctrl-shift-right` join the media keys in
  `answering_anywhere`, so one pair always steps the queue, caret or not.
- **A search is left for its results from the keyboard, and the albums grid is reached too.** With the
  caret in the search box, `down`, `tab` and `enter` are `GoToTheResults`, `TabOnward` and the field's
  own `Submitted`, all landing on `RootView::go_to_the_results`: the caret goes back to the window and
  the first row of whatever the pane lists is reached, so the next `down` steps through what was found
  and `enter` plays or opens it. `tab` is bound under `SEARCH_CONTEXT` to outrank the window's own, and
  from any other field `tab_onward` is the `focus_next` it always was. A query that moves drops a reach
  left in a listing, the rows under it no longer being the ones it was put on, so `down` after typing
  starts at the top. `Listed::Albums` makes the grid a listing the reach keys answer: they step the albums
  in reading order, a page being as many whole grid rows as the pane shows, `show_row` scrolls the grid
  row holding the album, `enter` opens it, and the reached cell wears `reached_ring` — an accent border
  laid over its cover, costing the grid no room.
  `down_and_tab_in_a_field_are_the_fields_own_before_they_are_the_windows` pins the two bindings.
  `Listed::Favourites`, `Listed::Missing` and `Listed::Suggested` carry the reach into the favourite
  tracks, the Missing pane's rows and an opened suggestion's rows: `enter` plays a favourite or suggested
  track from there, and on a Missing row opens the album it is short of or the artist whose release it
  is, a disc heading answering nothing. The Missing pane's two tabs are two listings under one `Listed`,
  so switching lets go of the reach (`let_go_of_the_reach_in`) rather than leaving it on the other's row.
  `reorder::marked` is generic over what it marks, a Missing row being a plain `Div` inside its card. All
  three are narrowed by the search, so typing at them searches rather than jumping. `Listed::Offered` and
  `Listed::Heard` reach the two panes that are no `uniform_list`: the Suggestions pane's cards in shelf
  order (`in_shelf_order`, kind by kind), where `enter` opens the card, and the Statistics pane's three
  tables read as one run, tracks then albums then artists, where `enter` plays the most-heard tracks from
  the one reached or opens the album or artist. Neither has a list's `scroll_to_item`, so `show_row` sets
  `RootView::reached_unseen` and the reached element carries `kit::brought_into_view`, a canvas that on
  its next prepaint moves the pane's `ScrollHandle` just far enough to hold it and spends the flag, so a
  wheel turned afterwards is not fought. A page is what the viewport holds — whole rows of cards, or
  `row_height` rows — and choosing another statistics window, like opening a card, lets go of the reach.
- **The window binds the media keys too, as a fallback rather than the feature.** A desktop grabbing
  the transport keys consumes them and calls MPRIS — how they are meant to work, and what makes them work
  with the window behind everything; where a session grabs none, the key reaches the focused window and
  the binding answers. gpui names an unmapped keysym by lowercasing `xkb_keysym_get_name`, so they are
  bound as `xf86audioplay` (the one play/pause key most keyboards carry, toggling), `xf86audiopause` to
  `Pause`, `xf86audiostop` to `Stop`, `xf86audionext` and `xf86audioprev`, and `XF86Back` and
  `XF86Forward` are two gpui does map, to `back` and `forward`. All seven are in `answering_anywhere`, a
  key on the keyboard's transport row not stopping because the caret is in the search field. `app.rs`'s
  test pins the seven names against `Keystroke::parse`, a name gpui cannot read being a binding bound to
  nothing rather than an error.
- **Typing at a list no search narrows jumps through it instead.** The search narrows the tracks,
  albums, artists, playlists-index and opened-playlist panes, so typing at them is what a listener means.
  The queue narrows on nothing, so `RootView::typed` routes a keystroke there to `TypeAhead` before the
  search arm: `jumped` prefers a row *starting with* the letters over one merely holding them and wraps
  at the end, `RootView::jumping` is the one place saying which panes opt in and how to read their rows,
  and the landing is the reach keys' `reach_at` and centred `show_row`. The pill says **JUMP TO** or **NO
  MATCH** in words rather than a colour, a colour being unreadable in a screenshot and a failed jump
  needing to be legible; it has no id and no listeners, so gpui inserts no hitbox and it blocks nothing
  under it (held `HELD_FOR`, 1100 ms). `typing.rs` holds the arithmetic with no gpui, tested as
  `Following` and `Step::landing` are. It folds through `resonate_library::folded_letters` — the search
  box's and `artists.key`'s fold — so *przybylowicz* reaches *Przybyłowicz* at the queue as in the search
  field. **The names a jump reads are read off the render thread.** `QueueNames` holds the queue's
  titles against the `Queued::revision` read at, and `names_in_the_queue` hands them out only while that
  revision stands; otherwise it asks the background executor — `queued_rows` reads the whole queue by
  id in one `Library::tracks_with_ids` pass of 500-id batches, a row whose id names another file by
  path, then `Player::media`, then the stem, the queue's own row order — and a keystroke before they
  land is still taken as a jump,
  `jump_where_typed` making it once they do. The queue pane asks as it draws, so the names are usually
  there before the first letter. They were once read on the first keystroke, on the render thread, at up
  to two SQLite reads a row through a cache smaller than a long queue.
- **Backspace drops a typed letter before it drops a reach, and never drops the row a jump landed on.**
  A jump *sets* the reach, so the other order would have made backspacing a mistyped letter take out the
  row the jump just landed on. `drop_typed` answers false whenever nothing is live, and the pill lapses
  after `HELD_FOR`, so a typo noticed a second late would still have taken the row: `jumped_to` notes the
  reach it set in `landed_by_a_jump`, and while the reach is still that one in the pane in front
  (`stands_where_a_jump_landed`) backspace does nothing. Any reach made by hand — the reach keys through
  `reach_by_hand`, a press through `reach_at`, a move, a drop — forgets it, so a row reached deliberately
  after a jump is backspaced away as ever, and `delete` takes a jumped-to row, being no typing key.
  Escape clears a live type-ahead after the magnified cover and the notice and before `dismiss_search`:
  those two stand until taken down while this one takes itself down after a second, and somebody
  abandoning a jump has not asked for their search to be cleared.
- **Typing anywhere still searches, which is why it does not take focus.** `RootView::typed` appends to
  the field without focusing it, so `space` stays play/pause until a click puts the caret in — except
  while typing is live: `space` is `PlayPauseUnlessTyping`, which `is_typing` turns into a typed space
  for `HELD_FOR` after the last letter or while a jump is live, so *pink floyd* is a search and not a
  pause, and the hardware play key keeps `TogglePlayPause`, which never types. Stop, shuffle and repeat
  are `ctrl-s`, `ctrl-h` and `ctrl-r`: as bare `s`, `h` and `r` they fired on every letter of a
  search, typing *Rush* stopping the music
  (`typing_at_the_window_searches_with_every_letter_and_its_spaces_rather_than_steering`). `space` and
  the three are bound under `!Search && !Control`, and so are `left`, `right`, `ctrl-left` and
  `ctrl-right`, which the transport would otherwise take from the caret, and the reach, move, undo and
  redo keys. So are the four that reach what was once the pointer's alone: `shift-right` and
  `shift-left` seek `SEEK_FURTHER_SECONDS` (30) where the plain arrows seek five, `ctrl-m` is the
  speaker's `toggle_mute` and `ctrl-u` the queue button's `toggle_queue` — held to control-letters and
  shifted arrows because a bare letter here is one the search can no longer be typed with. An action fires before any `on_key_down` listener and stops propagation, and a keystroke
  reaching neither is what the platform hands the input handler, so a key bound with no predicate could
  never reach the query. **Which predicate a key carries is structural, not per-line**:
  `answering_anywhere` (no predicate), `answering_away_from_a_field` (`!Search && !Control`),
  `answering_where_the_caret_is` (the control context) and `answering_on_a_band` (`BAND_CONTEXT`, the
  equaliser's) are four lists and `bindings` is their concatenation with `field::bindings`, so a new key
  goes in one of them and `every_key_not_named_to_answer_anywhere_is_dead_while_a_caret_is_held` walks
  the second and holds each to a predicate dead in a field and on a control and alive at rest. That test
  is what `up` and `down` lacked: they carried volume with no predicate, so volume moved while a name was
  typed — and why volume is now `ctrl-up` and `ctrl-down`, the plain arrows being the motion a hand
  reaches for on a list, which they now move. The editing keys live in `field::bindings` under the same
  context. Escape blurs and clears, closing the naming row before clearing a search; a press anywhere
  outside blurs through `on_mouse_down_out`. `ctrl-f` is the one key taking focus deliberately, hence
  bound with no predicate: it reaches the field from inside too, and selects what is there so the next
  keystroke replaces the query rather than extending it.
- **Escape away from the field is a chain, and a notice is in it.** `RootView::typed` hears escape
  outside the search field, `LeaveSearch` being bound under `SEARCH_CONTEXT` alone: an action fires before
  any `on_key_down` listener and stops propagation, so binding escape under `!Search` too would take the
  key off the handler answering it. The order is the match arms, what stands over the pane before what
  stands beside it: the Listen sheet closes first, then a magnified cover shrinks, then the playlist
  picker closes (`stop_naming`), then a shown record is cleared, then an open menu closes, then a
  standing toast is taken down through `toast::dismiss`, then a live type-ahead stops (`stop_typing`),
  and only then does `dismiss_search`
  close the naming row, clear the query, or — the query already empty — step back through
  `RootView::step_back`, the seam the page's way-back presses. That last step makes a scope escapable:
  opening an artist from the tracks pane leaves the pane and narrows it, so without a key the only way
  back was a heading button. The chain backs out innermost first, so a scoped search takes two presses —
  the words, then the scope. That holds only because typing is not a gesture on the scope:
  `LibraryModel::set_query` leaves `selection` alone, so a word typed into an open album narrows the
  album, and the callers meaning the whole library — `revise_search` and `search_instead`, which *Did you
  mean*, *Sung in*, a suggestion's *Search for these* and Listen's *Find it* go through — call
  `show_everything` before writing the words. A click on the toast is the same `toast::dismiss`. The
  caret keeps escape for itself: a focused field never reaches the match, `editing` returning first, so
  escape in the box does what it always did. `editing` asks `RootView::text_fields`, the one list of
  every `Field` the root holds, the Subsonic and TIDAL accounts' included, so a field cannot be left out of it
  as the ListenBrainz token once was, every letter typed into it landing in the search too
  (`what_is_typed_into_the_listenbrainz_token_stays_out_of_the_library_search`). Each field still
  needs its own arm in `dismiss_search`; the AutoEq search's, `leave_looking`, clears it and its
  results and hands focus back, where escape there once cleared the library search or stepped back a
  pane (`escape_in_the_autoeq_search_clears_it_and_leaves_the_pane_where_it_was`).
- **What stands over the pane holds the pane's keys back.** A menu, the playlist picker, a magnified
  cover, a record card or the Listen sheet is `something_stands_over_the_pane`, and while one does the
  reach, page, widen, move, `enter`, `delete`, undo and redo keys and every typed letter or backspace
  answer nothing: focus returns to the window wherever the pointer or a closed field leaves it, and
  the pane behind was otherwise edited through the sheet. The check sits in each gesture rather than
  on the bindings, so a button or a future caller is held back too; `reach_row` and
  `play_reached_rows` step and press the menu first, the menu answering those keys itself. Escape is
  the one key still heard, taking the sheet down (`views/root.rs`'s driven tests hold each sheet).
- **A slider is grabbed on the press and followed from a window-wide surface, not the rail.**
  `views/slider.rs` owns both rails: the press starts a `Grab`, and `drag_surface` — an absolutely
  positioned child of the app that occludes while held — carries the move and release listeners, a
  rail's own `on_mouse_move` firing only while the pointer is over those few pixels. It must be a
  *child*: `chrome::frame` adds its own `stop_propagation` move listener to the app element, and one
  element's listeners run in reverse order, so anything registered beside it in `render` would never see
  a move. Volume applies on every move and the `VOLUME_SETTLE` debounce keeps a drag from rewriting
  `config.toml` per pixel; a seek only previews and commits one `Command::Seek` on release, so a scrub
  costs one gap. The wheel over the volume cluster is a notch of `VOLUME_A_NOTCH` — a touchpad's pixels
  counted in `PIXELS_A_NOTCH` — while `ResonateApp::scroll_volume` says so, and `RootView::volume_aimed`
  is what a run of notches or held keys adds to: the engine publishes the new volume a poll later, so
  reading it back each notch lost all notches but one in a poll. **A press on the speaker, or `ctrl-m`,
  mutes, and mute is not a volume.** `RootView::toggle_mute` sends `Volume::MUTE` and keeps the level it was heard
  at in `muted_from` without storing anything, so a run quit while muted opens at the chosen level, not
  nothing. `muted_at` reads the pair only while the engine still publishes nothing, so a volume set over
  the bus ends the mute; a second press, a wheel notch, `ctrl-up` and `ctrl-down` all restart from the
  kept level, and grabbing the rail is a volume of its own and forgets it. Where the device has the
  slider (`OutputStatus::device_turned`) the press is the device's own mute instead —
  `Command::SetDeviceMute`, flipping what `device_muted` says — so a balance set in the desktop's
  mixer survives it and the desktop's mute switch agrees with the mark (`audio.md`). The release is taken from three
  places — a release inside the window, `on_mouse_up_out` for one outside, and a move reporting no button
  held — a pointer leaving the window mid-drag otherwise never being told to let go. The equaliser's
  handles ride the same surface: a `HeldBand` beside the `Grab` makes it occlude and follow, and one
  release lets either go (`eq.md` has the curve's side).
- **A drag leaving the window stops being followed, and that is the compositor's line.** gpui reports
  motion only while the pointer is inside the window, so a slider carried past the edge freezes at the
  last inside value and lets go on the way back rather than tracking to the rail's end, and a text
  selection drag tracks anywhere inside and stops at the edge. `on_mouse_up_out` still hears the release;
  nothing hears the motion.
- **`PlayerModel::state` hands back a borrow of the model, not a copy.** It borrows `cx` for as long as
  the reference lives, so a pane reads the model once and clones what it needs —
  `let model = self.player.read(cx); let state = model.state().clone();` — before anything taking a
  `&mut Context`. Writing `self.player.read(cx).state()` inline then calling `now_playing` or an entity's
  `update` is a borrow error, not a matter of taste.

## The design system

- **`theme.rs` is the palette and the scale, and `views/kit.rs` the vocabulary every pane is built
  from.** A palette is a `Flavour`: the surfaces `background`, `surface`, `raised` and `hover`, the
  `border` and `outline` between them, `text`, `muted` and `faint` over them, the `pitch` and `paper`
  either of which may be written on an accent fill, the `scrim` a sheet is laid under, the `alarm` the
  close control turns, seven `Accents` and the `native` one of the seven the palette was built around.
  Which flavour, which accent and which `TextSize` is worn is a `resonate_core::Appearance` — a `Theme`,
  an `Option<Accent>` and a `TextSize`, names with a text form and nothing else, in core because the
  config reader (compiled without `ui`) and the window both need it (the reason
  `MediaLocation::from_uri` is there), with not one hex in core — whose `None` accent is the palette's
  own, resolved by `Flavour::accent` and nothing else; `theme::wear` is the only way to set one. Every
  colour is read back through a function of its own — `theme::accent()`, `theme::background()` — so a
  pane names no constant and a theme change is one write behind one lock, not a value baked into every
  call site. **Every measure holding text is read back the same way**: `theme::row_height()` and
  `theme::text_base()` are `scaled` off the worn `TextSize`, so one setting moves the type ramp, rows,
  sidebar, header and every control together. What stays a `const` belongs to the compositor, not the
  type — `CORNER_RADIUS`, `RESIZE_BORDER`, the window marks, the rail's track and thumb — and
  `WINDOW_MIN_*`, read once when the window opens. **What is written on an accent is chosen by contrast,
  not a lightness threshold.** `theme::ink_over` weighs the flavour's `pitch` against its `paper` and
  answers whichever reads better on the colour given, so a generated palette needs no hand-picked ink
  and a deep accent is written in paper, a pale one in pitch. It takes a colour rather than reading the
  worn accent, `kit::dot_swatch` drawing its check on an accent not yet worn. The semantic colours come
  off the worn accents rather than a second list: `bit_perfect`, `lossless` and `done` are its green,
  `repacked` its blue, `dithered` its amber, `failure` its red, `suspect` its peach, `converted` and
  `lossy` the muted grey, so a colour means one thing in the playback bar's signal path, the inspector's stages, a row's format
  badge and a device's badge whatever palette is worn. `theme::tinted` makes a colour a wash — a
  selected chip, the playing row, a badge's ground — so no second palette of washes is kept in step, and
  `theme::selection` is the accent worn thin.
- **Every theme is dark, and the contrast tests hold every one readable.** Sixteen: Resonate's own warm
  near-black, Midnight, Graphite, AMOLED and Plum, Rosé Pine and Rosé Pine Moon, Catppuccin's Mocha,
  Macchiato and Frappé, Nord, Gruvbox Dark, Tokyo Night, Dracula, One Dark and Solarized Dark — each
  carrying the same seven accents, Mauve, Blue, Teal, Green, Amber, Peach and Red (`Accent::ALL`),
  under its own hex, so an accent is part of a palette rather than laid over it. **A fresh install
  wears the palette's own accent, not one colour named for all.** Every `Flavour` declares a `native`
  accent — Nord's frost blue, Gruvbox's amber, Catppuccin's and Rosé Pine's mauve — `Appearance::DEFAULT`
  names none, and the Colour card offers it as a ringed swatch ahead of the seven, so switching palette
  moves the accent until one is chosen by hand. Choosing none again is `Setting::Accent(None)`, taking
  the `accent` key out of the file as a cleared contact does. Sixteen also fills the shelf: the theme
  cards are `theme_swatch()` wide in a `settings_column()` body, four to a row, so palettes are added four
  at a time and every row stays full. **AMOLED's panes are pure black**, `background` and `surface`
  alike, so the pixels behind the lists are off on an OLED panel; what is raised and hovered still steps
  up, and it wears Resonate's own accents. **Where a published palette names no hue for an accent, its
  own terminal mapping decides**: Dracula has no blue, its ANSI blue being its purple and its magenta its
  pink, so those are its Blue and Mauve and it is built around the purple. One Dark's `abb2bf` reads at
  6.6:1 on its ground, so its body text is the palette's highlighted `d7dae0` and `abb2bf` is muted;
  Solarized's `base0` reads at 4.8:1, so its text is `base2` and muted `base1`, and its violet, orange and
  red carry their ink at 4.5:1 only on a pitch of pure black, which its pitch is rather than a lifted
  accent. `theme.rs`'s tests walk every theme crossed with every accent — and none — and hold contrast to
  the recognised bars: 7:1 for text on the panes and cards, 4.5:1 for text on what is raised, for
  `muted` on the panes and on what is raised, for `faint` on the panes and for what is written on an
  accent fill, and 3:1 for the accent against the panes, with the three inks stepping down from `text`
  through `muted` to `faint` on the ground. `faint` is text — a count, a key, a year, a placeholder —
  not decoration, so it is held to the body bar: where a published palette's own step reaches it the
  step is quoted (Catppuccin's `overlay2` is faint and `subtext0` muted, Frappé's `subtext1` muted, its
  `subtext0` reading at 4.3:1 on `surface0`; Gruvbox's `fg4`; Solarized's `base0`), and elsewhere the
  palette's faint is lifted along its own hue until it reads, the ramp's `THIRD_INK` included. Nord's
  aurora red is the one colour that could not carry its ink at 4.5:1 and is drawn a fifth of the way
  towards `nord6` so it can — what the guard is for, rather than trusting a famous palette.
- **A palette is quoted or ramped, and hand-writing twenty is how one drifts.** A quoted `Flavour` is a
  published palette's own hexes written out as a `static`, keeping Catppuccin's Mocha Catppuccin's; a
  ramped one is `ramp` over a `Recipe` — a native accent, a hue, a chroma, an alarm and a `LitAt` — run
  through a fixed lightness ladder placing the ground, what sits below, what is raised, the hover, the
  hairline, the drawn edge and the three inks, then generating all seven accents at the canonical hues.
  Midnight, Graphite and Plum are each a recipe rather than twenty colours, and Graphite is Midnight at a
  chroma of nothing. A ramped flavour is a `LazyLock`, not a `const`, turning an HSL triple into a packed
  `u32` being arithmetic a `const` context will not run; `hsl` is held to the cube's six corners by a
  test, so the ladder cannot quietly shift under its palettes. The three share the seven *hues*, an accent
  being what a colour *means* here — green is bit-perfect wherever drawn — but each lights them its own
  way through its `LitAt`: Midnight's pale and soft on the navy, Graphite's greyed towards its ground,
  Plum's deep and rich, so they no longer wear one swatch row between them
  (`each_ramped_palette_lights_the_accents_its_own_way`), the contrast tests holding every one.
- **Two faces, figures always in the second.** The window names Inter for text and JetBrains Mono for
  anything a listener compares across rows — clocks, rates, counts, bitrates, kbps, the inspector's tag
  values — and `theme::mono` builds the `Font` with `tnum` on, so figures line up down a column;
  `kit::figure` is the one way to draw one. Neither face is embedded, so **which family is drawn is
  settled once against what the machine has**: `fonts.rs` holds a ladder per face and `fonts::settle`
  walks it against `cx.text_system().all_font_names()` as the application starts, answering the installed
  spelling, so a machine without Inter draws Adwaita Sans or Cantarell rather than whatever fontconfig
  resolves. Neither ladder offers a family the other does (a test holds it), so a body face can never be
  taken for a mono one; a machine with none keeps the wanted family and lets gpui fall back. Not a
  setting and no key — a key with no control is what the settings rebuild went hunting — and the About
  card reports which two it settled on (`fonts::drawn_in`). `theme::text_xs()` through `text_title()` and
  `text_lyric()` are the whole type scale, set through `text_size` rather than gpui's rem steps, so the
  root's `text_size(theme::text_base())` is what a bare `div` inherits.
- **A control is a `kit::button` in one of four tones, and nothing draws its own.** `Tone::Primary` is
  the accent fill, the one gesture a heading leads with — *Play*; `Tone::Outlined` the raised secondary —
  *Play next* and *Add to queue*, one pair wearing one tone wherever they stand together, *Save as a
  playlist*, *Show graph*; `Tone::Destructive` the raised fill edged and lettered in the failure colour,
  worn only by a gesture losing something for good — the delete dialogue's *Delete*; `Tone::Ghost`
  everything else. Each carries an icon slot and a hint.
  `kit::icon_button` is the square version a row's controls and the playlist index's actions use,
  `kit::chip` the pill an order, reading, cap or setting is chosen from, and `kit::badge` the small mono
  tag a codec, a playlist's kind or a device's standing is drawn as. `kit::format_badge` is the codec
  badge beside the depth and rate spelt out by `format::quality` — *24-bit / 96 kHz*, never `24/96`,
  which read as a fraction to anyone new to it — coloured by `Codec::is_lossless`. The row's format cell
  is `theme::row_format()` wide, *32-bit float / 44.1 kHz* beside its badge being the longest it holds.
- **Whether a control can be pressed is the builder's decision, and a greyed one carries no press at
  all.** `kit::Press` is `Takes` or `Greyed`, `kit::button_when` and `kit::mark_when` are the builders
  reading it, and `kit::button` and `kit::icon_button` are those two under `Press::Takes` — so the
  thirty-odd ordinary call sites are unchanged. A `Greyed` control is drawn at `kit::GREYED`, takes
  `cursor_default`, and gets no hover style, pointer cursor or `icons::lit_on_hover`, so nothing under the
  pointer says it would answer. It must be decided where the element is built: gpui's `hover` carries a
  `debug_assert!(hover_style.is_none())`, so a second call panics a debug build and a control cannot have
  its hover painted on and taken off again. `settings::action` is where the pane spends it — taking the
  builder and the press *together*, handing the builder a `Press` and attaching the listener and focus
  ring only on the `Takes` branch, so `action` (*Add folder…*, *Rescan folders*, *Stop*) and the folder
  row's forget mark give it their listener and no call site can hang a press on a greyed control.
  *Rescan folders* is greyed by `LibraryModel::is_busy`, the answer `LibraryModel::scan` refuses on.
  **Adding and forgetting a folder are not greyed: they wait.** `LibraryModel::add_roots` and
  `forget_root` put what is asked into `roots_waiting` — a `RootWaiting::ToAdd` or `ToForget` — wherever
  a `Work` runs, the Folders group says what waits under its rows, and `take_up_what_waited` runs
  wherever a pass hands its `Work` back: a forget first, one at a time, then every folder waiting to be
  added in one scan. Before, a scan greyed both while it ran, so the second of two folders could not be
  added until the first was read. A forget mark is keyed by its root — an `ElementId::Path` under a
  `"forget"` child — not by one id every row shared. An enrichment is deliberately not a `Work`:
  `LibraryModel::enriching` is a slot of its own, so a running lookup greys the Online card's *Look up*
  and *Refresh all* and the Library card's *Enrich* through `held_back`, and a scan can still start under
  one. The run is visible from every pane: `RootView::enrichment_status` sits above the sidebar's
  *Settings* row only while `LibraryModel::is_enriching`, drawing the globe in `theme::faint()`,
  and *Enriching…* alone — no counts, which the listener asked to be taken off — ending in an ellipsis
  where the sidebar is narrower, and a press opens `Category::Online`. The Online card's *Look up*
  group says *Looking up…* on its button and nothing more while the lookup runs; what it found is
  the toast `looked_up` tells as it joins.
- **What a listener reaches for is what the lookup asks next.** `LibraryModel` holds one `Sought` for
  the run and hands it to every `enrich`, so a nudge made while nothing runs is still there when a run
  starts. Three gestures put something on it, all through `LibraryModel::ask_about`: scoping the tracks
  pane to an album, naming the album and its owner (`LibraryModel::album_anywhere`'s `artist_id`);
  scoping it to an artist; and playing a row, naming the started track's `album_id` and `artist_id`. It
  only reorders what the pass has left — an album asked yesterday is not asked again by a click — a nudge
  rather than a command; `library.md` has how the pass reads it.
- **Every pane opens with the same heading, and a page inside a category opens with a hero.**
  `kit::heading` holds a `kit::heading_row` — an eyebrow naming the section (*LIBRARY*, *COLLECTION*,
  *NOW PLAYING*, *SETTINGS*), a `kit::title`, a `kit::subtitle` with the counts and total length, and
  `kit::actions` on the right — and under it the *Reads* chips and a naming row where a pane has one.
  **The name keeps its room and the actions drop under it.** Every name block holds
  `theme::heading_name()` at least and `kit::heading_row` wraps, so where the name's least and the
  actions do not fit side by side the actions take a line of their own under it, up to the whole row
  and wrapping inside it: a long playlist name is not ground to a letter by thirteen controls, and no
  action is pushed off the pane. The actions once wrapped inside 64 % of the row beside a name free to
  shrink to nothing; at the 720 px window the queue's *Save as a playlist* ran off the edge, and the
  settings pane's 400 px find field, wider than its 64 %, spilled left over the subtitle. **An album or artist is not a pane heading with a picture bolted on**: it
  is `album_page_heading` and `artist_page_heading`, a `kit::way_back` at top left and a `kit::hero`
  under it — the album cover, the portrait and an opened suggestion's art each a square of the text
  column's own height, decoded at `Drawn::OnThePage` so a page hero stays sharp at scale 2. The column
  is the eyebrow, a `kit::hero_title` at `text_title` and the lines about it, aligned to the picture's
  bottom. Each picture is that height so it meets the text with no empty band beside it. The height is
  the column's own, read a frame behind by an absolute canvas filling it, so the inset resolves against
  the height the text already took; a zero-width sibling stretched beside the text came out a line taller
  than the column and left a band above the eyebrow. Until measured the cover uses `scope_cover`. The
  action row is the column's last child, under the text and clear of the cover: Play, Shuffle on an album
  or artist, the favourite and a sort icon where the listing sorts. Shuffle reads the same scoped listing
  as Play, begins at a time-chosen row and enables queue shuffle through `RootView::play_shuffled`, which
  the suggestions page uses too. **Play beside it plays in order**: every whole-list Play — the tracks,
  album and artist headings, an opened playlist, a playlist's index row and card, and a suggestion — sends
  `SetShuffle(false)` through `RootView::plays_in_order` before it loads, so a shuffle left on does not
  scramble the album pressed Play on. A row pressed to play from leaves shuffle as it is, and queues
  what Play would: `RootView::play_the_listing_from` plays the rows held where they are the whole
  listing (`is_the_whole_listing`, weighed against `listed()`) and otherwise reads it whole through
  `with_everything_listed` and starts at the pressed track, where a press or an Enter on row 1 990 of a
  20 000-track library once queued only the pages read so far
  (`a_row_played_from_a_listing_read_in_part_queues_the_whole_listing_from_that_row`). An album adds
  the info mark; an artist adds one where it has genres, and where it holds albums the Albums and Tracks
  choice sits at that row's right, so the listing follows the title with no band between. The releases it
  does not hold stay on the row too. Play next, Add to queue and Add to playlist are not on the page. The
  row does not wrap: a wrapping row measured to the column laid each button on a line of its own, taffy
  sizing a wrapping flex item at the column's min-content width. The title has the whole column and wraps
  rather than clipping. **The hero's column is measured, what wraps in it having to be told its width.**
  `kit::measures_its_width` is a canvas writing `RootView::hero_width` a frame behind — the same shape as
  the album grid's `kit::measures_the_grid`, a builder of its own writing a `GridWidth` — and the title
  and services line take it through `kit::hero_title` and `kit::wraps_within`. Until the first frame has
  measured, the title is one line ending in an ellipsis. The opened playlist uses the same hero, its cover
  the `views/mosaic.rs` art (the suggestion bullet below). Its text, a faint line saying when it was made
  and last played, and a left-aligned Play and Shuffle row sit beside the cover; then *Add songs*, *Sort*
  and *Tidy* for a list, the pin as a mark lit while pinned, and `Icon::More`, opening the index card's
  right-press menu — queueing, pin, rename, *Duplicate*, export and discard — at the press, so the page
  does everything the index can. The title opens the shared rename field, with a rename selector revealed
  while hovered; no separate rename button. The heading carries the same `kit::way_back`, reading
  *Playlists*, above an eyebrow of *PLAYLIST* or *SAVED SEARCH*. **A title that is a link is
  `kit::linked_title`, never `kit::title` over an `opens`.** The lyrics, inspector, visualiser and
  analysis headings name the playing track through `opens`, and under `kit::title`'s block box the link
  was laid out at no width, the heading drawing its ellipsis alone; `linked_title` is the same face as a
  flex row holding the link `keeps_its_width`, the by-line's artist's shape. **Such a link is cut
  before it is drawn**, gpui never truncating a no-wrap text past its first measure:
  `RootView::playing_heading` is the eyebrow, title and artist the lyrics, visualiser and analysis
  headings share, measuring itself into `heading_room` a frame behind and cutting both names there with
  `kit::cut_to_fit`, and the inspector cuts its title at `inspected_room`. Uncut, a long title was
  sliced by the pane's edge with no ellipsis. `by_line` cuts the artist at its room as well, the album
  taking what is left, so an artist longer than the panel ends in one too. The artist under it is
  `kit::linked_subtitle` for the same reason and a second: `kit::subtitle` over an `opens` stretched the
  link across the whole column, so a press well right of the name still opened the artist.
- **A track row is the same eight cells wherever drawn.** `listing::columns` is the header the tracks
  pane, the queue and an opened playlist put over their rows, and `number_cell`, `title_cell`,
  `artist_cell`, `format_cell`, `heard` and `length_cell` the cells under it, so no two disagree about a
  width. The header fills the same width as a row, so its growing title cell leaves the artist and later
  columns where the row puts them; its trailing space matches the controls its rows draw, an oversized
  blank cell squeezing the title and shifting every later column left. Title and artist give way
  together: `title_room` and `artist_room` both start from `theme::row_artist()` (200 px) and both shrink,
  only the title growing — a `flex_1`-from-nothing title beside a fixed artist was the first thing a
  narrow window took, to the last letter, while the artist kept all two hundred pixels. A track with other
  copies carries its `+N` *inside* the title cell — `tagged_title_cell`, the title `flex_1` beside it — so
  the tag takes room from the title rather than pushing every later cell along, and both text cells end in
  an ellipsis rather than a square cut. The playing row draws `listing::playing_mark` in the number cell
  and its title in the accent. A row's controls sit in `browser::row_controls`, invisible until hovered.
  **A listing narrower than its columns gives up HEARD, then FORMAT, before the names.**
  `listing::Shown::within` weighs the width the header measured into `RootView::columns_fit` — a
  `listing::Fitting` — against the fixed cells, `NAMES_AT_LEAST` for title and artist together and the
  controls that listing draws; `columns` publishes what it chose and every row reads it back, so header
  and rows cannot disagree, and a row is laid out with the `COLUMN_GAP` and `ROW_INSET` the arithmetic
  counts. An unmeasured header draws every column. Before, fixed cells a pane had no room for ran past its
  edge — LENGTH clipped to *LENGT*, the header's FORMAT over the rows' titles — while title and artist
  were ground to nothing (`a_narrowing_listing_gives_up_what_was_heard_then_the_format_before_the_names`).
  Track rows keep their Play next and Add to queue controls; the tracks heading carries Play and Shuffle
  for the whole listing and no Add to playlist.
- **Whatever a gesture takes out of the queue is kept, and a toast says so.** A playlist edit answers
  with a `Notice` and an undo; the queue answered with rows simply gone — the same gesture with none of
  the safety. `TakenOut` is what the last took — the rows, the row they started at and the ids the queue
  was left holding — and `RootView::took_out` is what the offer stands on. `drop_rows` is where it is
  written, so a row's ✕, a reach of thirty rows and backspace all keep what they take; *Clear* is the same
  call over `Span::between(0, last)`, not a `Command::Remove` of its own. `TakenBack::keeping` answers how
  many rows it kept, and `drop_rows` tells that as a toast — *Took 34 tracks out of the queue* — rather
  than a line under the heading standing as long as the offer did; *Put back* stays in the heading and
  sends the rows as a `Command::Insert` at `Placement::At` the row they came out of. The rows are kept as
  `QueueItem`s, not ids, `one_id_each` minting new ones on the way back in.
- **Dropping rows leaves the reach on a row that is still there.** `drop_rows` weighs how many rows
  the list held before the drop, and `Reach::after_dropping` lands on the first row after the gap —
  what moved up into it — or the row now last where the drop took the end, and nothing where it took
  every row. The reach used to stay on the dropped span's first row, which past the end of a shorter
  list drew no mark while `reach_in` pulled the next `delete` back onto the last row — a row that was
  never highlighted.
- **A run of gestures is walked back one at a time, and nothing queued since takes the walk away.**
  `TakenBack` is the bounded stack `TakenOut` sits in: *Put back* pops the top and sends its rows as a
  `Command::Insert` at the row after the one they followed — `TakenOut::after`, the id of the row before
  them when taken, which `landing_in` finds wherever it now stands, the front where they were the front,
  and where they came out where that row has gone too. A step stands while none of its rows is back in the
  queue — `stands_over` weighs their ids, which `Unclaimed::claim` hands back to a row on its way in — so
  an album queued, a row dragged or a track played between a *Clear* and a *Put back* leaves the offer
  standing where it once took it away, and a row already queued again is not put back twice.
  `KEPT_GESTURES` bounds the walk at sixteen, oldest first out, and the button's hint says how many are
  behind.
- **What was put back can be taken out again, walked forward as it was walked back.** *Put back* moves
  the popped step onto a second stack, and *Take out again* pops that: `TakenOut::standing_in` finds the
  step's rows by their ids as the run they were put back as, wherever edits have since moved it, and sends
  `Command::Remove` over that run, stacking the step so *Put back* offers it again. A step whose rows no
  longer stand together is not offered, and a fresh gesture clears the forward walk, as a new edit clears
  any redo. It goes through neither `drop_rows` nor a toast, being the gesture already told about made
  again. **The walk has the undo keys while the queue is in front**: `RootView::undo_edit` and `redo_edit`
  try `put_the_queue_back` and `take_the_queue_out_again` first where the pane is `Pane::Queue`, reaching
  the playlists' undo only where the queue has nothing to walk.
- **The offer is drawn while there is a step to walk.** An emptied queue is the one place the offer could
  be seen, so the heading is drawn over the empty pane while a step stands, *Clear* and *Save as a
  playlist* hidden there, having nothing to act on.
- **A card leaves out what the file does not declare, and the cards stack in two columns.** `fields`
  takes an `Option` per row and draws only what is there, one faint line where a whole card is empty —
  `tags`' old shape, shared by all five, so a file with no ReplayGain no longer renders five rows of
  *unknown*. The five were `flex_1` in a `flex_wrap`, stretching every card in a row to the tallest and
  leaving the last row ragged; they are dealt into two `flex_col` columns stacking tightly and collapsing
  to one in a narrow window.
- **The inspector is the one pane reading the catalog rather than the engine, and the *Heard* card is
  why.** Every other card is the `StreamDigest` or `OutputStatus` — what decoder and sink say about the
  playing track — but how often and when last a track was played are the catalog's, said nowhere else on
  screen. The reading comes off the `Track` `Playing` already resolves through `LibraryModel::track_of`,
  so the card costs no query: `Heard` is three fields carried on `Playing` (`heard_card`), and a row no
  scan has seen answers *no library row* rather than zero, a different thing. `format::since` is how the
  two dates read — the largest span that fits, *3 days ago* rather than a stamp.
- **What a search matched is lit where it is drawn, by one element.** `listing::matched` answers the text
  itself where nothing is lit and a `StyledText` carrying `HighlightStyle` runs where something is, so an
  unsearched row costs no more and a lit one still truncates, wraps and measures as one string. The runs
  are `Search::lit` for that cell's `Column` (`library.md`), so a name reached by a fold lights the
  spelling it is drawn in: typing *przybylowicz* lights *Przybyłowicz*. The browse panes are the only
  callers, being the only listings a search narrows: a track row's title and artist, an album cell's title
  and artist, an artist row's name. The queue and an opened playlist pass no runs. The accent lights a
  run — the playing row's title colour — with `FontWeight::MEDIUM` behind it so a run still reads where
  the whole title is accented.
- **The albums pane is a grid, its column count following the window in the frame it moves.** A cell is
  `theme::grid_cover()` square with title and artist-and-year under it, and a `uniform_list` row is
  `columns` of them, so two thousand albums still cost a screen of cells a frame. The width the columns
  are read from is `RootView::grid_width`, a `kit::GridWidth`: the canvas under the list
  (`kit::measures_the_grid`) writes on prepaint how much *narrower than the window* the grid is, and
  `RootView::region` tells it the window's width whenever the pane is built, so it answers the window's
  width now less that — sidebar and paddings being fixed, a resize is the same pixels on both. When what
  it is narrower by moves, the canvas asks for the next animation frame, a refresh asked mid-draw being
  ignored. **The list is not built until that cell holds something**, as the lyrics pane's `place`
  reveals nothing until the measured size is the size it has: the `max(1)` fallback is a column count
  nobody wants, and full-width cells stacked one a row is the most visible thing the pane can do. The
  canvas is `absolute` and sized by the container, not the list, so it measures the same bounds whether
  the list is there or not, which makes holding it back safe. A resize draws at the new count the first
  frame it is drawn at (`a_grid_is_as_wide_as_the_window_now_less_what_it_was_narrower_by`); only a change
  the window's width does not carry — a sidebar grown with the text size — takes the measure's frame. A
  cell's cover is `Drawn::InAGrid`, one of the four `Drawn` sizes (`InARow`, `NowPlaying`, `InAGrid`,
  `OnThePage`), sized by `Scale::texels_for`.
- **An empty pane says what it is missing and what to do about it.** `kit::empty` draws the pane's own
  icon in a ring, one sentence naming the state and, where there is one, a second saying which gesture
  fills it, so "No albums yet" is followed by where to add a folder and an empty queue by where a row
  comes from.
- **A search that matched nothing offers the spelling the catalog holds, and the offer is a press.**
  `kit::empty_offering` is `kit::empty` with one more child, what the three browse panes'
  `RootView::nothing_matched` hands it: a `Tone::Outlined` button reading *Did you mean Billie Eilish?*
  that puts that text in the search field through `RootView::search_instead`, the field's own observer
  taking it from there — so the correction goes in the box, editable, rather than being run behind the
  listener's back. It draws `LibraryModel::instead`, which `browsed` fills only where the albums, the
  artists, the tracks and the sung matches all came back empty, so the offer cannot
  contradict a pane listing something; where there is no spelling to offer but the words are sung
  somewhere, `nothing_matched` offers *Sung in N tracks* through `sung_offer` instead. `library.md` has
  how the spelling is found. All three panes ask through one method, all three emptying on the same
  search, and the albums and artists panes build it before the heading, the model's borrow having to end
  before `cx.listener` takes one.

## Drawing

- **A plot is quads, never a path.** gpui 0.2.2 rasterises every batch of vector paths through a
  window-sized 4× MSAA texture cleared and resolved each frame, which cost an integrated GPU far more than
  the plots — and the scope, the inspector's bitrate graph, the analysis pane's waveform and spectrum and
  the equaliser's curve are all plots. `views/plot.rs` draws them out of `paint_quad`, which gpui batches
  as instances: `stroke` lays one quad over each segment's bounding box, as tall as the segment rises plus
  the line's width and never narrower than it; `wash` one from each segment's middle height to a level —
  the floor for a bitrate or spectrum, the zero line for the equaliser's signed curve, so a cut is washed
  up to it rather than down to the floor; and `between` one per column from the higher of two traces to
  the lower, the waveform's envelope. A gradient is laid over each quad rather than the plot, so it runs
  from the trace down; points being a pixel or three apart, the steps read as the line. `PathBuilder` has
  no caller in the crate, and nothing is drawn through the MSAA texture.
- **An icon is an embedded SVG reached by an `Icon` variant, never a text glyph, and one list is the
  whole of what an icon is.** The `icons!` macro takes a variant and the name it is drawn from, writing
  the enum, `Icon::ALL`, `Icon::path` and `Icon::drawing` from that one list, so a variant cannot be added
  to the enum and left out of the array the `AssetSource` walks — which once compiled, answered
  `Ok(None)` and rendered as nothing no test caught, `path` and `drawing` being exhaustive and the test
  iterating `ALL`. `crates/resonate-ui/assets/icons` holds one silhouette per variant, embedded by
  `include_bytes!`, and `icons::Embedded` is the `AssetSource` the `Application` is built with, so a path
  nothing answers cannot be written and the error enum needs no missing-asset variant. `Icon::Resonate`
  is the application's own mark and the one icon with no file under `assets/icons`: the header's wordmark
  fills the accent square with it, *drawn out of* `packaging/resonate.svg`, which carries the five paths
  under a `translate`-and-`scale`, over the rounded ground and in the gradient a launcher wants and an
  alpha mask cannot hold. `icons::masked` takes that file's `<g>` alone — the strokes, their width and
  caps — drops its `transform`, trades the gradient for a plain stroke and sets it on a 24-unit `viewBox`,
  once, behind a `LazyLock`, so the launcher's icon is the only place the mark is written. `icons.rs`'s
  test holds the mask to the packaged paths and refuses the ground, gradient or placing riding along, and
  reads `Icon=` out of the desktop entry to hold the asset's name to it, as `app.rs` holds it to
  `APP_ID`. gpui renders an SVG to an alpha mask tinted with the element's `text_color`, so an icon
  carries no colour and inherits nothing: each is given a colour at the call site, and
  `icons::lit_on_hover` makes one follow its button's hover through a group. Every one is therefore a
  silhouette on a 24-unit box, and a two-tone mark — a state badge over an icon — would be two overlaid
  `svg` elements. No control is drawn with a text glyph: the window controls are the `Window*` icons, and
  the search field's clear mark, a row's ✕ and the queue row's arrows are `Icon::Close`, `ChevronUp` and
  `ChevronDown`, a control drawn to a box keeping its mark centred where a glyph's baseline did not. **A
  control's box and its mark are one declaration.** `theme::Control::of` takes the hit area and the mark
  inside it together and asserts, in a `const`, that the mark leaves `MARK_MARGIN_AT_LEAST` on every side
  — so a box shrunk under its mark or a mark grown past its box fails to build rather than drawing a glyph
  clipped at the button's edge. The measures stay functions — `row_control` and `row_control_icon`
  reading one `Control`'s two halves — and a control that borrowed another's mark has its own: the search
  field's clear, once an unscaled literal, is `CLEAR`; the settings filter's `FILTER_CLEAR`; the accent
  swatch's tick `CHECK_MARK`, not the hint's size.
- **The launcher's icon wears the accent, and the binary puts it there.** `AppIcon::of` is the whole
  decision, beside the palettes: an appearance whose worn accent is `Appearance::DEFAULT`'s colour
  answers `Packaged`, any other `Recoloured` — `packaging/resonate.svg`'s two gradient stops replaced by
  the accent lifted towards white and the accent itself, its root given `id="resonate-in-the-accent"` —
  so switching palette moves the icon with the accent until one is chosen, and going back to Resonate's
  own removes it. `AppIcon::drawn_here` reads that id, which makes the file ours to replace or remove:
  an icon under the name nobody here drew is left as it was — **unless it is a byte-for-byte copy of the
  packaged icon**, as a hand install of `packaging/` or an unpackaged tree run leaves there. Such a copy
  takes the accent as an empty place does, but what is written over it opens with
  `data-written-over="the-packaged-icon"` beside the id, so going back to Resonate's accent writes the
  packaged copy back rather than removing the file and leaving the launcher iconless
  (`a_copy_of_the_packaged_icon_takes_the_accent_and_is_put_back_after_it`, in the binary's
  `launcher.rs`). `run` shows the appearance it opens in and `RootView::dress` every one worn after,
  through `Launcher`, a seam on `Stored` as `Present` is, so `resonate-ui` does no I/O for it. The
  binary's `launcher::Icons` is behind it: a thread waiting `SETTLES_AFTER` of quiet so a run of presses
  is placed once, writing a staged copy over `$XDG_DATA_HOME/icons/hicolor/scalable/apps/resonate.svg`
  or removing it, and only where that moved anything flushing what the install scriptlet flushes — the
  theme folder's mtime, `kbuildsycoca6` and KIconLoader's `iconChanged`
  (`resonate_mpris::tell_the_icons_changed`) — and, where the folder holds an `icon-theme.cache`, the
  GTK cache through `gtk-update-icon-cache` or its GTK 4 twin, a stale cache hiding the new file from
  every GTK desktop. A run opening in the appearance the file already shows writes and flushes nothing.
  **The gradient is in `userSpaceOnUse`**, spanning the mark's 24-box: under the default
  `objectBoundingBox` each stroke is a zero-width box, which the SVG spec says paints nothing, and
  librsvg drew the packaged icon as an empty tile.
- **The catalog is read while gpui starts, so the first frame already holds it.** `run` starts
  `FirstRead` before `Application::new`: a thread reads what a model with nothing chosen asks for —
  `Asked::at_first`, which `LibraryModel::new` is built from too, so the two cannot drift — while gpui
  spends forty-odd milliseconds of the main thread loading every system font and bringing Vulkan up.
  `LibraryModel::new` takes it from `ResonateApp`, and the first read takes it synchronously where it
  landed, awaits it where not, and loads afresh where what the model now asks differs from what was read
  ahead. On a 616-track catalog the first frame with the albums in it finished 105 ms after `main` where
  it had at 112–122, and no frame is drawn empty first; the rest of that start is gpui's own font scan and
  device creation, beyond reach.
- **`resonate-codec` decodes and scales cover art, and gpui is handed the pixels.**
  `Drawing::no_larger_than` and `Drawing::squared` answer a `Raster` — width, height and pixels already in
  the blue-first order gpui's atlas holds — and `models::picture_of` wraps it in a `RenderImage`, so a
  cover is decoded once, by us, never written back out as PNG for gpui to decode again. A tag-catalog
  picture arrives the same way, through `CoverArt` re-exported by `resonate-engine`. Each is read once and
  held in a `Recent` keyed by album, artist or file *and the side drawn at*, a fresh `Arc<RenderImage>`
  every frame being a new texture every frame. Read and decode run on `Drawer`'s two threads rather than
  the background executor: a miss puts the key in the model's `decoding` set and hands the work over, the
  cell draws its placeholder disc until the picture lands, and the landing notifies the model, so a
  screen of unread covers costs the render thread nothing and a picture a file lacks is cached as a miss
  rather than asked every frame. A miss holds only until the catalog next changes: `LibraryModel::reload`,
  which a scan and a lookup both end in, forgets every cached `None` cover and portrait
  (`Recent::forget_where`), so a picture a lookup fetched or a rescan found is drawn rather than the
  placeholder kept for the run, while the pictures already drawn stay
  (`a_cover_found_after_it_was_first_drawn_is_drawn_once_the_catalog_reloads`). **A file's picture is cached only once the engine settled it.**
  `Player::art` answers nothing on the ask queueing the read, so `PlayerModel::art` reads
  `Player::art_read` — `NotYet`, `Answered` or `Nothing`, `TagsRead`'s shape — and a `NotYet` is kept in
  `unsettled` against the `media_revision` it was asked at rather than in `pictures`, asked again once the
  revision moves; caching that first `None` as a miss left the playback bar's cover empty for any file the
  catalog does not cover, even one carrying its own picture. `LibraryModel::warm_the_covers` fills the
  album cache ahead of the grid once a run: `COVERS_WARMED` (256) albums declaring a picture, drawn at the
  grid's side and decoded one at a time in a task of its own, so a scroll into a library already has its
  covers. One at a time is how it stays out of the way — never more than one decode queued, so an
  on-screen cell waits behind at most one warming cover — skipping what the cache or in-flight set holds,
  and bounded under `COVERS_HELD` on purpose, warming more than the cache keeps evicting its own work.
  `DECODES_AT_ONCE` bounds how many are in flight, in the album cache and the per-file one alike: a cell
  asking while the bound is full is refused *without* being marked decoding, so it asks again next frame
  rather than being lost, and a landing notifies, guaranteeing that frame. Without it a fling queued one
  decode per newly visible cell onto an unbounded FIFO — every core busy on covers scrolled past, ahead of
  those on screen.
- **A cover is decoded on one of two threads of its own, glibc giving every allocating thread its own
  arena.** A 1280-pixel progressive JPEG costs ~15 MB while decoding, and a block that size stops being
  mmap'd once glibc's dynamic threshold has risen, so it is kept by the arena of whichever thread freed
  it. Spread over gpui's sixteen workers that was ~110 MB held after the start-up warm — a window at
  265 MB where the same window decoding on `Drawer`'s `DRAWING_THREADS` (2) settles at 158, its peak
  falling from 280 to 158. Two threads keep up with what the four-wide executor did, a cover now ~14 ms
  rather than 27. `Drawer` is a gpui `Global`: `draw` queues a closure and answers a future (a
  `futures-channel` oneshot) the model awaits, and a queue nobody reads — threads unable to start — runs
  the closure where asked rather than dropping it. The allocator was weighed and lost: mimalloc (v3 and
  v2) and jemalloc each settled higher than glibc — 575, 470 and 345 MB — a worker that decodes then idles
  never giving back what its thread cache holds.
- **What a cache lets go of, gpui is told to let go of too.** A picture is a `RenderImage` the cache owns,
  so `Recent::insert` hands back what it pushed out or overwrote as a `Leaving`, and `Forget` turns that
  into `App::drop_image`, taking the picture's tiles out of every window's sprite atlas — for album
  covers, portraits, per-file pictures, the magnified picture a new one replaces and the spectrogram a new
  painting replaces — so what the GPU holds is bounded by what the caches hold, where a cover drawn once
  stayed in the atlas for the run. A forgotten picture still on screen is only decoded again. The one
  picture still handed to gpui as encoded bytes is the Listen sheet's, which `Image::remove_asset`
  forgets.
- **A cover is drawn at twice the device pixels it is shown at, and at that side alone.** gpui's atlas
  sampler is bilinear with no mipmaps, reading four texels however far it reduces: a 1280-pixel cover in a
  22-pixel cell was point-sampled and crawled. `Drawn` names the cell — `InARow`, `NowPlaying`,
  `InAGrid`, `OnThePage` — and `Drawn::side` asks the window's `Scale` for the texels
  (`Scale::texels_for`): twice the cell's device pixels below a scale factor of two, so the sampler's four
  texels are exactly the 2×2 it should average at 1, 1.25 and 1.5 alike, and the device pixels themselves
  from two on, drawn 1:1. `RootView` hands the scale to both models whenever the window's moves, and a
  picture is keyed by its drawn side, so a window dragged to another display draws its covers again at the
  new size rather than stretching. Nothing is drawn at a size nothing asked for: a picture is decoded for
  the one cell wanting it, so a library browsed as rows never pays for the grid's covers, at the cost of a
  second decode where one album shows in two cells at once. Against a true area average over this
  library's covers, twice the device pixels is ~sixteen times closer than handing the sampler the whole
  cover, where a 128-pixel thumbnail was only a third closer. The scaling is `CatmullRom`, run in linear
  light, not on sRGB values, and a cover already smaller than the cell is handed over as it came.
- **The resize is `image::imageops::resize`'s arithmetic walked in memory order.** The crate's vertical
  pass fixes a column and walks down it, a strided read per tap per pixel, converting every tap to linear
  light as it went — two thirds of a cover's decode. `artwork::drawn_smaller` computes the same weights
  with the same expressions and sums each output pixel's taps in the same order, so the result is the
  crate's to the bit (`a_cover_is_drawn_exactly_as_the_image_crate_would_draw_it` holds it to `resize`
  over random RGBA) — but it adds a whole source row into a column of sums at once, converts each source
  row to linear light once into `Held`, a ring as deep as the widest tap window, and runs the horizontal
  pass straight from that column, building no intermediate image. A JPEG stays the `RgbImage` it decoded
  to rather than being copied into RGBA, and `squared`'s crop is a `Plane` over the same samples. The
  transfer function is a 256-entry table, not a `powf` per channel per pixel — exact, the input being an
  8-bit channel; the 8-bit hold costs a 16-bit PNG's intermediate precision, quantised on the way in and
  invisible in a thumbnail written back out as 8-bit.
- **What the magnifier shows is read when it opens, not kept beside every thumbnail.** Holding the
  source bytes beside each cached picture so the magnifier could answer at once was a copy of the
  original per cached entry, for a view showing one at a time. `LibraryModel::whole_cover` and
  `PlayerModel::whole_art` read it on the way in, drawn no larger than `MAGNIFIED_AT_MOST` on a `Drawer`
  thread, keeping one `Magnifying`: the key being read, then that key with what it read, so a key
  answering nothing is not asked again every frame (`magnified_art` being read each frame the magnifier
  is open), and a read landing after another picture was asked for is dropped from the atlas. It draws
  its scrim and title and no picture until the read lands, and nothing else in the window pays for it.
  Nothing magnifies a portrait. With the covers no longer holding their source, a window on a 435-track
  library of 1280-pixel covers fell from 689 MiB to 264.
- **A portrait is a picture cut square, held apart from the covers.** `drawn_square` draws through
  `Drawing::squared` where a cover draws through `no_larger_than`: the centred square of the shorter side,
  scaled down to the side asked and never grown, so the sampler gets the square the round frame shows.
  `LibraryModel::portrait` takes a `Portrayed` — `InARow`, `InAGrid` or `OnThePage` — whose `Drawn` sets
  the side (a now-playing size never asked), reading from `portraits`, a
  `Recent<AtSide<ArtistId>, Option<Picture>>` under `PORTRAITS_HELD` (256) beside the 512 covers, drawn on
  `Drawer` under the same `DECODES_AT_ONCE` bound with `decoding_portraits` as its in-flight set, and only
  where `Artist::has_portrait` says there are bytes. `browser::portrait_frame` is the round frame —
  `ObjectFit::Cover` inside a `rounded` div — at `theme::avatar()` in the artists list and
  `theme::scope_cover()` in the scoped heading, and `kit::avatar` draws where there is none.
- **The magnifier's scrim occludes, so the press that closes it closes it and nothing else.** An
  `absolute` child painted over the whole app, whose hitbox without `occlude` was one more hitbox rather
  than a wall: the press shrinking the cover also landed on whatever row, cell or control stood under the
  pointer, so closing a cover over the albums grid opened the album beneath. The menu's scrim already
  did this; the magnifier now does too.
- **A cover is drawn wherever a row names a track, out of one cell.** `RootView::cover` takes a
  `Pictured` — an album, or a track (an `Option<AlbumId>` beside its file) — so the albums, tracks, queue
  and playlists panes draw the same 22-unit cell. The album's art is preferred and `Player::art` is the
  fallback, putting a picture on a row no scan has seen; a file carrying none loses only the picture.
  `RootView::drawn_cover` answers the art and a `Magnified` naming its source, so the clickable
  transport cover opens whichever of the two it drew. The overlay painting over the whole app is sized
  from `Window::viewport_size`, not a constant — the shorter window edge, capped
  (`theme::magnified_cover`), never filling a wide screen — and nothing pages from one cover to the next.
- **The window holds decoded pictures apart from the bytes they came from.** The window's two models
  hold 256 per-file pictures (`PlayerModel`, `PICTURES_HELD`) beside 512 album covers (`LibraryModel`,
  `COVERS_HELD`), in `Recent`s bounded by count where the engine's `ROWS_HELD` and `ART_BYTES_HELD`
  bound the bytes, so neither window cache is bounded by what a picture weighs. A `Recent` keeps a
  `BTreeMap` from its use clock to the key beside the entries, re-keyed whenever one is looked at, so
  evicting is `pop_first`, not a scan for the minimum — the engine's `Held` shape, keeping a scroll past
  the 4 096-row bound (`ROWS_HELD`) from walking four thousand entries per newly visible row. A `Recent`
  evicts the entry nothing looked at rather than emptying itself, so a row scrolled well past either
  bound is read again when it returns — up to two SQLite reads the first time a queue row is seen, by id
  then path. Nothing empties `LibraryModel::named` on a library read, so what a counted play changes
  about a track is put back by hand: `track_heard` writes the row `Library::track_played` answered into
  the cache, keeping a queue row's count moving when the row is in no listing the reload re-read.
- **Text answering the pointer is lit when the element is built, gpui being unable to light it later.**
  gpui 0.2.2 resolves a `hover` style only in *paint* — `compute_style_internal(None, …)` in
  `request_layout` and `prepaint` — while a text child is shaped in `request_layout` with the colour of
  the moment baked into its runs. So a `hover` changing a background works and one changing `text_color`
  or adding an underline never reaches a glyph: every link `opens` drew, the column headers, a ghost
  button's label and a segment's all promised a hover nothing painted. `views/pointed.rs` is the answer:
  an element following the pointer carries an `on_hover` writing its `ElementId` into `POINTED_AT`, a
  thread-local set, and asks the window to redraw; the next build reads `is_pointed_at` and shapes the
  text in the lit colour, as a selected row's already is. `LitUnderThePointer::lit_under_the_pointer` is
  the whole gesture — an id and what to do to the element while pointed at — and `follows_the_pointer`
  its half for an element lighting something inside it, how an album cell lights its title. A set rather
  than one slot, a cell holding a link: the pointer over the artist under a cover is over both.
  `set_pane` and `show_everything` empty it, a link pressed to go somewhere being taken away while
  pointed at and never hearing the pointer leave, and the pointer leaving the window empties it through
  `pointer_watch`. A control with a background keeps its `hover` for that, only the text going through
  `pointed`.
- **A hint names its key one way, and the key is the one bound.** `keys.rs` is `key!`, the spelling of
  every key a hint names — `key!(play_pause)` is `space`, `key!(raise_row)` is `alt-up` — and
  `app::bindings` binds through the same `key!`, so a hint cannot name a key the window does not answer.
  `keyed!` writes the one wording: what the key does, an em dash and the ways to do it, the last two
  joined by *or* and any before by commas — `Louder — the wheel or ctrl-up` — and a hint saying more
  than one thing is such sentences joined by full stops, a described state first: `Repeat is off. Repeat
  the queue — r`. Both are `concat!` all the way down, so every hint stays a `&'static str`.
  `every_key_a_hint_names_is_a_key_something_is_bound_to` walks the bindings for each.
- **A control names itself through one seam, and the seam lets go.** `hint::Names` is the only place a
  tooltip is attached, a test walking the crate's own sources to keep it so. It is gpui's plain
  `tooltip`, never `hoverable_tooltip`, `clear_active_tooltip_if_not_hoverable` being a no-op for the
  hoverable one: neither a press nor a scroll takes it down, and every hint here sits on something
  pressed or in a list scrolled. `hint::asking` is the other half — one atomic written once a frame by
  `RootView::render` (`hints_are_wanted`) — and where it says no, no builder is attached, which gpui
  reads as an instruction to drop a live tooltip on its next prepaint. It says no while a slider is
  grabbed, an equaliser band is held, the picker, a magnified cover or a record is up, a row is dragged,
  the pointer is outside the window, and for the one frame in which what lies under an unmoved pointer
  did move: `RootView::laid_out` stamps the pane, the settings category, the queue's revision, the three
  listings by their `Arc`s, the playlists and the landing list's scroll offset, and
  `UnderThePointer::moved_since` weighs the stamp and the pointer's position against the frame before —
  so a row removed from under a still pointer, a pane stepped to from the keyboard or a list scrolled by
  a key takes down the hint naming what was there, where gpui, hit-testing only on a mouse event, would
  have left it over the control that took its place. The reported defect: gpui's `MouseExited` arm is
  the one not updating `Window::mouse_position`, so a pointer leaving the window leaves the hint
  believing it is hovered, and the 16 ms poll draws it there for the run. `RootView::pointer_watch` is a
  zero-size canvas registering `MouseExitEvent` and `MouseMoveEvent` window listeners from its *paint* —
  `field.rs`'s `follow_the_pointer` idiom, and the only phase `Window::on_mouse_event` may be called in
  — and the flag it sets also closes the lyrics pane's hover, so a pane opened out by the pointer does
  not stay open once it has gone.
- **Every control names itself in a tooltip, and a tooltip wants a pointer.** The playback bar, the
  sidebar rows, the window controls, the queue row's arrows and cross, every action in the tracks and
  playlists headings, each row's queue marks, the search field's clear mark, the info mark holding the
  search grammar and a folder's forget mark all carry one, and a toggle names its state and the key
  changing it. A list row says what it is only through the text it draws, so a keyboard-driven session
  sees none of it.
- **A playlist row no scan has seen draws itself as a queued one does.** `views/listing.rs` owns the
  `Row` the queue and playlists panes build: the scanned `Track` where the library has one,
  `Player::media` where not, and the file stem only where the file cannot be read. That lets an imported
  playlist of unscanned files read as titles and artists rather than stems, costing `resonate-ui` no new
  dependency — the tag catalog already being what the queue pane draws from. The play count rides on the
  same `Row`, so a queue row and a playlist row draw it where a track row does, out of `listing::heard` —
  one `theme::row_plays()`-wide cell the tracks pane, both list panes and the playlists index all use, so
  no two disagree about its width or words. A row no scan has seen carries a count of nothing and draws an
  empty cell, what the catalog already says about a file it never met.
- **The cell says how often and how lately, the catalog being ordered by both.** `played:` narrows and
  `SortOrder::Played` orders, and neither had anything on screen to read against: when a track was last
  heard was drawn for the playing track alone, in the inspector's *Heard* card.
  `how_often_and_how_lately` folds the date into the count's own cell rather than taking an eighth
  column from rows carrying seven — *12 plays · 3d* — and the header reads HEARD rather than PLAYS, the
  word the inspector's card, `listing::heard` and `COUNTS_AS_HEARD` use for the pair. `format::age` makes
  it fit, the `SPANS` table `format::since` reads written in its short spelling: one `Span` carries the
  singular, the plural and the brief, so a span cannot be named one way and forgotten the other
  (`every_span_is_named_both_ways_and_no_two_read_alike`). A row nothing played draws the empty cell it
  always did, and a count with no date beside it — as a playlist's own reading can be — draws the count
  alone.
- **A catalog listing marks the playing track by the row it resolves to, never the queue's id.** The
  tracks pane, Favourites and an opened suggestion compare against `RootView::playing_now`'s `track`, the
  library row `LibraryModel::track_of` weighs out of the queue row's location and span. They compared the
  queue row's own `TrackId`, the library's id only for a row loaded from the catalog this run: a resumed
  queue is minted afresh by `Queue::restore`, so after a restart pressing Play lit nothing. What
  `track_of` read is held in `named`, and the albums the bar links to in `read_albums`, only until the
  catalog next changes: `LibraryModel::reload` forgets both, so a title, artist, album link, favourite or
  *not in the library* a rescan or a lookup changed is read again rather than kept as first read
  (`a_queued_row_is_read_again_once_the_catalog_changes_under_it`).
- **The row playing is the row a list is on, never a track id.** A file can be in a queue or playlist
  twice, and an unscanned row has no `TrackId`, so both panes ask which row the transport is on. The queue
  draws `PlayerState::queue_position`, the row of the published play order; a playlist draws
  `PlayerState::loaded_position`, the row of the load order — the playlist's own order however shuffle
  moved the play order. A playlist marks a row only while `Library::playing_playlist` names it, which it
  does only while the queue holds the rows it was loaded with: the mark is sound exactly as long as the
  queue *is* that playlist, and a row added or dropped — by the window or over the bus — ends that.
- **An edit never cancels the one before it.** `LibraryModel::edited_then` holds one task in `_edit`, and
  replacing it dropped an edit spawned a moment earlier before it ran — its write, its toast and its
  reload gone. Each new edit takes the one before out of the slot and awaits it first, so edits run
  in the order asked and none is lost (`two_edits_asked_for_at_once_both_land`); `track_passed` and
  the two `listened` writes on `_settled` chain the same way, a settled play's length no longer dropped
  by the next play's first hearing.
- **A narrower read never cancels a wider one.** A read replaces `_load`, dropping the one in flight —
  right between two whole reads, but it lost the browse panes where a playlist edit's `ThePlaylists` read
  landed on top of a scan's closing `Everything`: the listing kept the rows the scan had just pruned.
  `in_flight` is what the model remembers, and a read asked while one runs reads what both would have
  — `Wanted::with`, `Everything` over anything, two of a kind as they are, and any other pair
  `TheSearch`, which covers both
  (`a_read_asked_for_while_another_runs_covers_what_both_would_have_read`).
- **A read of the library says how much it wants.** `LibraryModel::read` takes a `Wanted`:
  `Everything` reads the browse panes, what stands whatever is typed — the statistics, the most
  listened, the days, the suggestions and the roots (`Standing`) — and the shelves: the playlists, the
  wants and the missing (`Shelves`). `TheSearch` reads the browse panes and the shelves and leaves the
  standing — a keystroke, a selection, a sort and a favourite, none of which move it, so a search no
  longer asks for the week's statistics each time. `ThePage` reads the four lists `reach` bounds —
  albums, artists, tracks, the scoped tracks (`Paged`) — alone, for `reach_further`, their counts and
  every other pane standing still. `ThePlaylists` reads the shelves alone. Every gesture behind
  `LibraryModel::edit` writes to `playlists`, `playlist_entries` or `playlist_queries` and nothing
  else, so an edit, a listing-order change and opening a playlist take that; a scan, a counted play,
  a window of time and forgetting a root take the whole. `Loaded` carries each part as an `Option`, so
  `take` tells what came back empty from what was never asked, and `renewed` swaps a list's `Arc`
  only where what came back differs, so a read that moved nothing leaves every pane's rows, the
  album index and the chart where they stood
  (`a_list_read_again_the_same_is_left_where_it_stood`).
- **A read the typing asked for waits for the typing to stop; every other runs at once.**
  `LibraryModel::set_query` is `read_after`'s one caller, and `SEARCH_SETTLE` is the 150 ms it holds the
  read, so ten characters cost one pass over albums, artists, tracks, the playlist index with its
  `count(*)` per saved query and the opened playlist's entries, not ten. What a keystroke does move is
  `Search::read` and the query beside it — a handful of tokens — so the *Reads* row and a pane's own
  narrowed reading are live as typed and only the SQLite work settles. The wait lives in the `_load` task,
  so a keystroke inside it drops the one before rather than queueing a second.
- **Every notice is a toast: a pill that rises over the playback bar, lingers and goes.** It is the
  cousin project wsg's `chrome::toast`, and `toast.rs` is the whole of it. `Toaster` is a gpui global,
  not a field of any model, so the engine's failures, the library's passes, the equaliser and every
  gesture tell it alike — `toast::tell(notice, cx)` from anywhere holding an `App` — and `RootView`
  redraws on `observe_global`. One pill stands at a time, centred over the content `type_ahead_lift`
  above the bar and lifted clear of the type-ahead pill when showing; it rises `RISE` (10 px) and fades in
  over `ARRIVAL` (220 ms), lingers `LINGER` (6 s) — `LINGER_IN_A_BURST` (2 s) where more wait, at most
  `WAITING_AT_MOST` (4) waiting and never the same words twice — and fades out over `WITHDRAWAL` rising
  the same 10 px, a click or escape taking it early. A `Notice` is `Trouble`, `Done` or `Noted`, and the
  tone is the pill's icon — the alert in the failure colour, the tick in green, the info mark in the
  accent — the words staying the body text colour. `EqualiserModel` sets its notice where it holds no
  `cx`, so the notice is an outbox `RootView` drains into the toaster (`take_notice`) whenever the model
  notifies. One line stays in place, belonging to its control rather than the moment: the Library card's
  refusal of a typed layout field. **A finished pass is a toast too, never a line left standing.** A scan,
  an organise, a retag, a vault import and an inbox poll draw their counts in their settings
  group only while they run — `stats` and `poll_stats` answer `None` once over, a lookup drawing none
  — and the
  model tells what came of it as it joins: `scanned`, `looked_up`, `filed`, `retagged`, `vaulted` and
  `polled` in `models.rs`. A scan or poll the window started on its own is told only where it brought
  something, so a watch rescan finding nothing stays quiet. A *preview* keeps its summary in the group,
  being the plan the second press arms against rather than news.
- **A toast speaks plainly, and the error behind it goes to the log.** Nothing an error's `Display` says
  reaches the pill. The engine names what went wrong as `resonate_engine::Cause` — `Unreadable`,
  `Unsupported`, `Damaged`, `NoDevice`, `DeviceGone`, `SoundServer`, `DeviceRefused`, `CannotSeek`,
  `QueueMoved`, `NothingPlaying` and `PlayerStopped` — read exhaustively off the codec's and PipeWire's
  errors in `Error::cause`, the window seeing neither crate, and `Error::location` names the file a
  failed read was of, so a queued row that failed and left the queue before the next poll is still named:
  *Couldn't find “Gone Song” — it may have been moved or deleted*. `toast::would_not_play` and
  `toast::would_not_do` turn a cause into words, and a library or equaliser failure is
  `toast::could_not("start the scan", &error)` — *Couldn't start the scan — another library task is still
  running* — the reason after the dash only where the typed error says one worth saying. A notice is a
  label: no full stop. `looked_up` says *34 albums and 12 artists answered*, leaving the breakdown to the
  Online card, and the sidebar's enrichment line wears `faint` and `muted` rather than the accent.
- **The device name is not in the playback bar.** The sink's description was the one child of the signal
  path with no width of its own, so a long one pushed the panel to its clip edge and took the album title
  with it. The inspector's OUTPUT stage names the device and the settings row keeps its node name behind
  the pointer; the signal path's hint says *Playing through …* before what it always said.
- **The window is a thin root over four cached regions, so a moving clock redraws the playback bar and
  nothing else.** gpui re-runs the root view every frame and reuses the last frame's layout and paint only
  for a child drawn as a *cached* `AnyView` nothing notified. `views/part.rs` is that split: a `Part` is a
  view of its own for the header, sidebar, pane or playback bar, rendering `RootView::region` through a
  weak handle — so every pane is still a method on `RootView` taking `Context<RootView>` and its listeners
  bind to the root — and `RootView::render` is the shell: the actions, overlays, menus and toasts. Each
  `Part` observes the root, so a `cx.notify()` on `RootView` redraws all four as the one tree was, while a
  hover, a scroll or a frame asked inside a region notifies that region alone, gpui naming the view a
  listener was painted under. A cached view is laid out from a style, not its content, so
  `Region::laid_out` declares each one's size — header and playback bar at fixed heights, sidebar at its
  width, the pane taking the rest — held in a one-cell grid whose track is `minmax(0, 1fr)`, so a pane
  is never laid out wider than its region whatever least width its content asks, where a flex row
  honoured a pane root's automatic minimum. `PlayerModel::refresh` says what a poll moved as a `Moved`: `Clock`
  where only the position and the sink's latency changed — the `held_still` reading `Grain::shows` makes —
  and `More` otherwise. The root's observer notifies itself for `More` and hands a `Clock` to
  `Parts::the_clock_moved`, notifying the playback bar and, where `Pane::follows_the_clock` — the
  inspector's latency and the analysis pane's playhead — the pane. At a scale factor of 1.5 on a
  2880×1800 output, a playing window on the tracks pane went from 4.2 % of a core to 2.4 %, on the albums
  pane from 6.8 % to 2.4 % and on the inspector from 15.5 % to 3.2 %. **How often the clock moves is the
  `Grain`** (`EveryPoll` or `Stepped`), what the render tells the model the moment is drawn at: every poll
  on the visualiser, whose spectrum follows it, and while a seek thumb is held; elsewhere a quarter of a
  device pixel of the seek rail — `STEPS_PER_PIXEL` over `Rail::width` times the scale factor — never
  coarser than the clock's second (`CLOCK_STEP`), so elapsed time turns over when it should and
  `Listening` and `Keeping` never see a step near the `A_SEEK` they read as a seek. The lyrics pane is not
  every poll: a synced set asks a frame at the display's rate while it moves, and a pane with nothing
  moving has nothing a poll would change — forcing one was a whole-window paint sixty times a second over
  an empty pane. `Grain::shows` lets anything but the position and latency through at once — a pause, a
  seek, a track change, the sleep timer's second — and an unpainted rail is every poll. The model's state
  is still refreshed every poll, so whatever else asks a frame draws the moment as it is.
- **The playback bar is three columns, the transport the middle one.** The now-playing panel and the
  status cluster are both `flex_1` with a zero basis and at least `SIDE_AT_LEAST` (180 px), taking equal
  halves of what the centre leaves, so the buttons sit on the window's centre line whatever is beside
  them. The centre is a column with a basis of `theme::transport_centre()` shrinking to
  `CENTRE_AT_LEAST` (260) — it gives way first: holding its 460 at the 720 px window it left each side
  86 px, the cover took all of the panel, and title, artist and album drew nothing. It holds the step and play buttons
  over the seek rail with the elapsed and total clocks at its ends; the rail fills its row
  (`Handle::fills_its_row`). Both sides carry `overflow_hidden`, a badge or notice wider than its half
  otherwise painting over the controls. **What does not fit is left out whole, never clipped.**
  `views/gives_way.rs` is the arithmetic, over the theme's measures with no gpui: the status cluster is
  measured into `RootView::status_room` a frame behind, as `playing_room` is, and `status_kept` drops
  the volume reading, then the sleep control, then repeat, then shuffle, then the volume rail, until the
  rest fits — a running timer weighed at its countdown's width — never the queue button (the one way to
  the queue pane) or the speaker (which still mutes and takes the wheel). `signal_kept` does the same
  for the signal path against `playing_room`, widths read off the font through `kit::width_of`: the
  mode's name goes first, then the depth and rate, then the mode's dot, the codec badge staying. An
  unmeasured column keeps everything. At 960 px the cluster had been clipping from its left edge —
  queue, shuffle and repeat gone, the moon cut in half — and the path cut mid-figure. The now-playing
  panel is the cover at `theme::now_playing_cover()`, the title, the artist and album, and under them
  the signal path: the decoding format badge with its depth and rate, then the output's mode dot and
  name in the mode's colour — *bit-perfect*, *converted* — and nothing after. It once carried a link
  mark and the negotiated depth and rate too, doubling the line to say what the mode's name and the
  inspector's *Output* stage say; what a converted stream became is the inspector's to spell out. It is
  the line the inspector's three stages expand on, and a press on it opens that pane, the short reading
  and the long one not being two unrelated places. Title, artist and album are three elements, not one
  truncated line: each opens the page behind it through `RootView::opens`, the title and album scoping
  the tracks pane to `Selection::Album` and the artist to `Selection::Artist`. Which is pressable is
  what is *known*: the album id rides on the panel's resolved `Cover` and the artist id on
  `Track::artist_id`, so an unscanned row draws all three as plain text, the names off the file's tags
  with nothing behind them. `RootView::by_line` is the artist, separator and album as one method, taking
  the two element ids to draw under, so the inspector's heading is the same line, not a copy. The
  by-line reads as one line because `keeps_its_width` holds the artist at `flex_none` under a
  `max_w_full`: three truncating children shrink in proportion to their own length by default, so an
  album title four times the artist's took a quarter of the loss from the artist — the album is what
  gives way, and the artist ellipsises only where it alone overruns the panel. **What gives way ends in
  an ellipsis, not a square cut, and each is given a width it can be cut at.** gpui's text element
  truncates only inside its measure and keeps a no-wrap text's first measure whatever width it is later
  handed, so a `truncate`d name inside a content-sized flex item is laid out whole and sliced by its
  parent's clip; a `line_clamp` on the same item is measured at its min-content width and drew *The
  Tr…*, or nothing. Title and album are both cut by `kit::cut_to_fit`, which cuts the text itself
  (gpui's `LineWrapper::truncate_line`), so the element draws the shortened string at its width. The
  title is cut at the width `RootView::playing_room` measured a frame behind, less the star; the album
  at the room `by_line` is handed less what artist and separator take (`kit::width_of` summing the
  font's advances), and a by-line with no room left draws the artist alone. The inspector measures its
  heading into `inspected_room` likewise. The album used to be `flex_1` under `ends_in_an_ellipsis`, and
  gpui 0.2.2 paints a clamped line's underline to the whole unwrapped run — a one-line clamp recording
  no wrap boundary to end it at — so hovering an album longer than its slot underlined the rest of the
  panel. The inspector's stage cards are `flex_1` already and take the clamp, and their row wraps: a
  pane that cannot hold three at `stage_width` puts the last under the first two rather than off its
  edge. A notice the engine raised
  is in neither half: it is a toast over the content. Every bar control names itself through
  `views/hint.rs`, a silhouette not saying what it does: a step names its key, a toggle its state and
  what the key does next, so the accent is not the only report. A rail is drawn filled in `TEXT`,
  turning to the accent under the pointer or while held, its thumb drawn only then. The volume's reading
  is always drawn, in `theme::volume_reading()` of room and the body face: at `opacity(0)` until the
  pointer was over the cluster it reserved the room without filling it and read as a hole between rail
  and window edge. `kit::readout` draws it — `theme::ui` is the body face with `tnum` on, so the figure
  is as steady as `kit::figure` without being mono, which a percentage beside a slider has no reason to
  be.
- **The play mark carries its optical nudge in the art, not a margin.** A right-pointing triangle
  centred by its bounding box reads a little left, its mass on the base, so `play.svg` (`M7 5.5 18 12 7
  18.5Z`) is drawn half a unit right of the 24-box's centre and the button adds nothing. It replaced a 2
  px `margin-left` on the play state alone, which put the triangle ~2.5 px right of the disc's centre
  and, `pause.svg` being dead centre with no margin, moved the mark sideways on every toggle. Anything
  an icon needs to sit right belongs in the icon.
- **A lyric look starts when the track does, not when the pane opens.** `look_for_lyrics` runs on
  `RootView`'s observer of `PlayerModel` — the one `count_a_play`, `Keeping` and `Following` ride — so a
  track change searches whatever pane is in front and the pane opens on a set rather than *Looking for
  lyrics…*. It costs nothing idle: `Asked` is `Copy`, `worth_looking_again` refuses a repeat, and the look
  runs on the background executor.
- **The whole sheet opens out wherever the pointer is on the column.** `near_the_words` decides — inside
  the centred column band, anywhere from the pane's top to its foot — taking bounds and answering a
  `bool`, so tested without a window. It once answered only within reach of the line being read, leaving
  the lines already sung dark under a pointer held over them — exactly where somebody points to read back.
  The view reaches it through the `pointer_watch` idiom: a zero-size `canvas` registering a
  `MouseMoveEvent` window listener from *paint*, the one phase `Window::on_mouse_event` may be called in.
  An `on_hover` on the pane opened the sheet from the empty gutters and either far end.
- **The lyrics pane follows the playing track and nothing else.** `RootView::wanted_lyrics` builds a
  `Wanted` from the queue row's location and the `StreamDigest`'s tags, filling title, artist, album and
  length off the scanned row until the digest lands — the album through `LibraryModel::album_title`, the
  length at the row's own rate — so the first look has all a provider searches on and a `[length:]` to
  weigh a sidecar against. A redraw compares `lyrics::Asked` — the track id, its duration and whether a
  digest for it landed — which is `Copy` and says all that could change what is wanted; `Wanted` is
  built only once `LyricsModel::asks_again` says the reading moved, so a 60 Hz redraw no longer clones
  title, artist, album and the whole embedded lyric text out of the digest to find them unchanged.
  `follow` resets the pane only where the *track* moved, so a digest landing mid-track refines the look
  without rewinding the scroll — and the lookup that refinement starts rewinds on landing only where the
  track moved or it answered a sheet other than the one shown (`already_shows`) — and
  `worth_looking_again` decides whether there is a look: the registry is asked again on a track change,
  and otherwise only where the `Wanted` differs from the one last searched on. `Asked` moving is not
  enough, the `tagged` flag flipping exactly once per track when the digest lands — so every scanned
  track, whose row already gave the title and artist the digest repeats, paid a second search that could
  only answer what the first had. An unscanned track still pays two and earns them: the first on the
  location alone, finding a sidecar, the second on what the file said it is. The look runs on the
  background executor, a network provider not to block the frame. The lines become `SharedString`s once,
  when the look lands, so a 60 Hz redraw hands each line a reference count rather than copying the sheet
  every frame.
- **The lyrics heading says where a set came from, and what the set says about itself rides behind the
  pointer.** `attribution` ends the heading: a `SYNCED`, `UNSYNCED` or `WORD-SYNCED` badge, the
  provider's own name as a `kit::figure`, and — only where the sheet credited anyone — one more figure
  naming who laid it down. Which name is the window's choice: the `[by:]` transcriber, the `[au:]`
  author of the words where nobody signed the sheet, and the editor where neither is named, so the row
  draws the most particular claim made and nothing where none was. The rest is that figure's hint,
  through `hint::Names`: the words' author, the sheet's, the editor with its `[ve:]` version and the
  `[al:]` record, each sentence written only where the sheet wrote it. One figure is all the crowding
  the row takes — Follow, two reading chips, the press hint and two attribution marks are already in it
  — so a sheet declaring everything reads as one name and hovers as four sentences.
- **The pane always draws and scrolls the whole sheet; a reading is only how far the light reaches.**
  `Falloff::Around` looks only forward — the line sung and the two after, everything sung out — which is
  `Reading::InPlay`; `Falloff::Across` steps down gently both ways so `Reading::Whole` stays readable to
  the edges (its tail at 0.16), the type bold and centred in its column. **The sung line brightens; it
  does not grow.** Every line is `Measures::words` in a box of `Measures::leading`, lit or not, so a row
  takes the same rows at the same height whatever the turn is doing. A size in motion cannot be drawn
  smoothly here: gpui on Linux puts a glyph on a whole pixel vertically (`SUBPIXEL_VARIANTS_Y` is 1, the
  origin floored) and cosmic-text hints every size, Inter carrying TrueType bytecode, so the line that
  grew from 26 to 36 px was re-hinted at each of its eighty steps — cap height and baseline landing on
  different pixels step to step, a shimmer through the turn and a one-pixel hop as it ended. Before the
  size was dropped, a row was measured at the size it would reach and its text laid out in a box scaled
  by how far it had grown, a line on one row at rest having taken two as it lit and slid the sheet under
  itself. A line is drawn for
  what is coming, not what has been: keeping the last one up behind the sung one makes a pane read as a
  transcript rather than a track playing, so `Falloff::behind` is `0.0` for `Around` and `ahead` mirrored
  for `Across`. Drawing every line either way makes the motion possible: an `InPlay` reading building a
  fresh three-line column each time would have nothing to scroll, lines swapping in place instead of
  sliding. The choice is a heading chip living only as long as the run, like the settings pane's
  category, and the pointer over the pane opens it out, as does a scroll of your own while it holds the
  pane — `LyricsModel::shows_every_line` being the one answer all three go through. It weighs
  `following`, which runs out on a clock, so `follow_the_track` turns `spread` onto it every frame and the
  sheet closes again over one `TURN` once `HANDS_OFF` (6 s) is out. An unsynced set has no lit line, so
  every line stands at `ADRIFT` and no chip is offered.
- **Two voiced lines have separate reading edges.** A set with a second voice places voice one on the
  leading side and voice two on the trailing side, with a small voice label when the singer changes. Text
  and alignment both identify the voice; the second also takes the accent when lit. Each voice's active
  line brightens on the existing turn, even where both sing together. A one-voice set keeps its
  centred column and former width.
- **The falloff counts written lines, not rows.** `drawn` records each line's ordinal among the
  non-blank ones when the look lands, so a blank line between verses costs its neighbours no standing and
  the reading is the same three lines across a verse break as within one; reaching for it per frame would
  be a walk per line per line.
- **Three `Turn`s on one `TURN`, and a `Glide` on a spring.** Where the pane *reads* and what it
  *lights* are not the same line, so each has its own clock. `Reads` is what the light turns on: `At` a
  line while one is in play, `Spent` where none is — carrying the line `read_at` names — and `Evenly`
  for an unsynced set never lighting one. **Nothing lit is not an empty pane.** Where nothing is in play
  but no wait has begun — a gap under a breath, a line whose sheet ended it just short of the next — the
  turn stays `At` the line just read: it keeps its standing and only its colour falls to `muted` as its
  light goes. Reading such a gap as `Spent` put every line out for as long as it lasted, the text
  vanishing for a second between two singers. `Spent` is a wait's: the set goes to `Falloff::spent`,
  which in an `InPlay` reading stands the line waited on and the one after it as though the line before
  were being sung — `ahead(1)` and `ahead(2)` under the dots, nothing behind — and across every line
  the same step down either way from the read line a line in play gets, that line standing where its
  neighbours do rather than lit, a deliberately asked reading staying readable. Putting everything out
  left a pause as dots alone, the line rising out of nothing; a flat 0.16 over the lot before that left
  the whole sheet all but unreadable past the last line however opened out
  (`a_gap_shorter_than_a_breath_holds_the_line_just_sung_rather_than_putting_the_sheet_out`,
  `a_pause_leaves_the_line_it_waits_on_and_the_one_after_readable_under_the_dots`).
  `read_at` keeps the sheet where it is, which is also what takes a set's last line away once it has had
  its word rather than leaving it lit through the outro. `standing` blends over `Reads`, `lead` over the
  lines in play, and colour reads off `lead` — `mixed` lerps `muted` to `text` — so a line brightens
  over one 420 ms (`TURN`) rather than snapping at a threshold. The third turn is
  `spread`, over the `Falloff` itself: the pointer opening the pane out and a reading chip both go
  through `Turn::onto`, so lines a wider reading brings up fade in over the same span, and `standing` is
  the falloff turn blended over the reads turn. The `Glide` differs in kind: it carries the scroll
  offset to the read line's `landing` over `GLIDE` (600 ms) on `landed`, `1 − (1 − s)³(1 + 3s)` — at
  rest at both ends, fastest a third of the way, never passing its landing, a move decelerating into it
  reading as the sheet arriving where an ease-in-out reads as it being pushed. It ends on a cubic, not
  an exponential: on whole pixels the last one comes about 35 ms after the one before. The critically
  damped spring it replaced crept its last pixel or two in 130–150 ms apart, long after the sheet looked
  still, which read as the sheet jumping a pixel at the last moment; an underdamped one before that came
  back from its overshoot the same way, as a flutter
  (`a_glide_lands_on_a_whole_pixel_with_its_last_step_close_behind_the_one_before`).
- **The sheet grows with its pane, and its type is still a size at rest.** `Growth::of` reads the
  pane's measured size against `PANE_AT_RESTING_SIZE` (784 × 600: the 720 px column and its two gutters,
  about ten rows down) and takes the tighter of the two ratios, held between 1 and `GROWS_AT_MOST` (2.5),
  so a wide but short pane does not grow type it then has no rows for, and a pane at or under the
  resting size draws as it always did. Every length of the sheet is `Measures::at(scale, growth)`: the
  type and the voice label, the leading, padding, spacing, a breath's room, a pause, the plain margin,
  the column and its gutters, the line's side padding, the dots, the end mark, the dissolving edges and
  how far a set rises — the Text size setting's scale times the growth, so the setting still means what
  it says on a 4K pane. The growth changes only when the pane does, so it is the layout's, not motion's:
  a turn never changes a size, which is why it sits beside "the sung line does not grow". A resize
  moves `Measures`, which `place` already weighs, so every line is laid out again before any is held
  open at an old height (`the_sheet_grows_with_its_pane_by_the_tighter_of_its_two_sides`,
  `the_column_widens_with_the_pane_and_never_outruns_its_gutters`).
- **Everything in the sheet moves on whole device pixels, because gpui draws text on them.** A glyph's
  vertical origin is floored to a device pixel while a quad is drawn where it is, so text given a
  fractional place stepped a pixel at a time while its hover wash and the dots slid. `Scale::snapped`
  rounds to the window's device pixels (`RootView::follow_the_scale` hands the lyrics its scale):
  `landing` is snapped before a glide leaves — an odd pane height put a centred line on a half pixel,
  which the glide crossed only in its very last frame — `glide` sets the offset snapped, and `inset`
  rounds each line's place *once*, as `snapped(where the line is) − snapped(where the sheet is)`. The
  offset and a line's lag rounded apart, the offset by the glyph floor and the lag by taffy, made their
  sum tick a pixel back and forth though both moved one way: 322 such reversals over a minute of a test
  sheet, which read as the lines vibrating
  (`every_line_is_drawn_on_whole_pixels_and_never_steps_back_through_a_glide`). `Measures` holds every
  vertical length of a row — the type, the words' leading, a voice label's, the padding, the spacing, a
  breath's room, a pause, the end mark's padding — snapped the same way at every growth, and the head is `edge()` snapped, so every row starts and ends
  on a whole device pixel. taffy rounds a size by where it sits (`round(top + height) − round(top)`), so
  a 62.08 px row came out 62 or 63 as rows above it changed, the read line moving a pixel with no line
  change (`the_measures_of_a_line_are_whole_device_pixels_at_any_scale`).
- **A line further down sets off later, so a change ripples rather than shifts.** Every row is two
  boxes: the outer is what `bounds_for_item` measures, and the inner is `relative()` with a `top` inset
  of `LyricsModel::inset`, so nothing the motion does moves the bounds the landing is computed from.
  `Glide::lagging` is the whole ripple: a line `n` written lines past the read line is where the sheet
  was `LAG_PER_LINE × n` ago, capped at `LAGS_AT_MOST`, its inset the distance from there to where the
  sheet is — so the lines below the sung one are still catching up as it lands, as Apple Music's sheet
  does and one offset for the lot cannot. Lines above the read line lag nothing, what was sung being out
  of the way first. `Glide::settled` therefore waits the last lag out, so frames keep coming until the
  furthest line is home.
- **A set arrives rather than appears.** `place` starts `arrived` the moment the pane is placed, and two
  readings come off it: `arrival`, the body's opacity, an ease over `ARRIVES_IN`, and `rise`, each line's
  inset, `RISES_FROM` down and falling to nothing on `landed` over `RISE`, `RISE_PER_LINE` later for
  each written line from the read line either way, capped at `RISES_AT_MOST`. Until placed, `rise` answers the full `RISES_FROM` and
  `arrival` nothing, so the first frame lays out low and invisible and the set comes up out of the read
  line. `breath` reads the same clock: a sine over `BREATH`, what the gap's dots swell on.
- **A look only just started is kept quiet.** `follow` stamps `asked_at` where the track moved on, and
  `looks_quietly` answers true for `LOOKS_QUIETLY_FOR` after while the look is still `Searching`; the pane
  draws an empty body then rather than *Looking for lyrics…*, a sidecar answering in milliseconds and a
  placeholder flashing between sets being the least seamless thing about a track change. It asks its own
  frames, so a look taking longer is announced when the grace runs out.
- **A gap is read at the line it waits for, not the one just sung.** `read_at` prefers `Waiting::next`,
  then the ended row, then `line_at`, so an instrumental scrolls the upcoming line to the middle and
  lets it rise there under the dots while all that was sung is out — and past the last line, with nothing to wait
  for, it falls back and holds.
- **`centre_of` reads the laid-out bounds, and a new landing carries the glide on.**
  `ScrollHandle::bounds_for_item` is what gpui laid out with no scroll offset, so the landing *is* the
  offset, and adding the current one would make the target chase the glide and never settle. A landing
  within `SETTLED` (half a pixel) of the held one is no move; any other `redirected`s the glide: a new
  `Leg` from where the sheet is, carrying the pace it had as `launch` on `launched`, `s(1 − s)³`, which
  starts at that pace and dies away on the same cubic tail, held to `LAUNCHED_AT_MOST` times the travel
  so the leg never passes its landing. The leg before stays as `Glide::before`, so a lagging line reads
  the path the sheet took. Lines sung faster than a glide then scroll as one motion: a glide started
  afresh from rest stalled at every line, and each lagging line jumped its whole lag as it restarted
  (`a_new_landing_in_flight_carries_the_glide_on_without_a_jump_or_a_stop`).
- **An unsynced sheet is placed at its top once and then left to the reader.** With no line read,
  `landing` answers the top for a synced set, waiting on its first line, and for any set not yet
  placed; a placed unsynced set answers nothing, so `place` leaves the offset where the wheel took it.
  Answering the top there glided a plain sheet back to its first line `HANDS_OFF` after every scroll
  (`a_plain_sheet_is_placed_at_its_top_once_and_then_left_where_it_was_read_to`).
- **A line far from the pane is not laid out, only held open at its height.**
  `LyricsModel::resting_height` answers the height `bounds_for_item` last gave a line wherever that line,
  moved by the scroll offset the bounds leave out, lies more than `DRAWN_WITHIN_PANES` — a pane's height
  — above or below the pane, and only while `LyricsModel::steady`: the pane's size and the `Measures`
  the same two frames running. A frame reads the layout before it, and a line held open in that layout
  was read off the one before that, so a change must lay every line out once more before any is held
  again; gating on the frame's own size alone let the sheet's first layout — taken with the pane zero
  wide, a letter a row, lines a thousand pixels tall — hold those heights for the sheet's life
  (`a_line_is_held_open_only_once_two_layouts_running_were_measured_alike`). `lyric` draws
  such a line as an empty box of that height and width, so the sheet's layout, the child indices
  `centre_of` reads and every other line's place stay as they were while a hundred unseen lines cost a
  box each rather than a text layout each frame. A line coming within a pane of view is drawn whole before
  it can be seen, and a pane changing size lays every line out once more.
- **The end of the words is a row of its own, so an outro is not an empty pane.** Once
  `Lyrics::has_ended` — nothing in play, nothing but blank lines to come — the read line is
  `LyricsModel::end_of_the_sheet`, one past the last, and the turn reads `At` it, so the sheet glides to
  an *END OF LYRICS* mark between two short rules `end_of_the_words` draws as the last child before the
  trailing spacer, lit while all sung falls away as a line behind the sung one does. `Sheet::written`
  carries one more ordinal for it, so it rises, lags and stands like any line: faint two lines ahead of
  the last word, lit once it has gone out. An unsynced set draws no mark, having no clock to end on.
- **A pane is drawn nowhere until placed, and a row is as wide as the pane says.** The sheet's spacers
  are measured off `ScrollHandle::bounds`, a frame behind, so the first frame lays out with no padding
  at offset zero and the second with the padding but centred off the first's child bounds — two frames
  of a sheet sliding into position being what "it glitches and then corrects itself" was. `place`
  therefore reveals nothing until `steady` — the size and `Measures` the layout was taken at being the
  ones it has — snaps the
  offset rather than gliding, and reports itself moving throughout so frames keep coming while paused;
  until then the body is drawn at `opacity(0)`, laid out unseen. `follow_the_track` snaps its turns over
  the same span, so a set arrives at its standing rather than fading in from `Reads::Evenly`. `rewind`
  puts it all back, so a track change places the next set afresh rather than gliding from where the last
  sat. Each row takes an *absolute* width, `LyricsModel::column_width` — the pane's measured width less
  `Measures::gutter` each side, capped at `Measures::column` — rather than `w_full` under a
  `max_w`: taffy fixes a flex item's height from a measure taken before a percentage width resolved and
  never measures again, so under `w_full` every wrapped line was laid out one row tall while gpui
  painted it wrapped, the next row drawing over its second. The head of a synced sheet is the
  container's own top *padding* and the tail a spacer *child* — not two half-measures: `content_size` is
  the children's extent alone, so a leading child would put every line a row off what `bounds_for_item`
  answers and leave the sung line a line's height below the middle, while a trailing padding would leave
  nothing to scroll into and the last line never reaching it. The pane asks its own frames — a zero-size
  `canvas` calling `Window::request_animation_frame` while any clock is in flight — the 16 ms poll
  (`POLL_INTERVAL`) notifying only when the player's state *changed*, so a turn started by a press, seek
  or chip while paused would otherwise freeze half way.
- **A press on a line is a seek, and a scroll of your own is not fought.** Every timed line carries
  `RootView::seek_to_moment`, clamping to the track's duration before `Command::Seek`, since the engine
  refuses a frame past the end and a hand-written `.lrc` whose last stamp overruns the file would leave a
  red line in the playback bar only a track change clears. A wheel over the pane marks it `led_by_hand`
  and the pane stops following for `HANDS_OFF`; the heading grows a *Follow* button while held off, and
  that press, a press on a line or a track change resumes it.
- **The pane reads a clock of its own, not the engine's position.** `PlayerState`'s position is what was
  decoded less what ring and device still hold, so it climbs a decoded block at a time — ~90 ms for FLAC
  — and drains in quanta between: a sawtooth, and everything keyed off it — the word sweep, the filling
  dots, the moment a line lights — moved in those steps. `LyricsModel::keep_time` runs a `Clock` on the
  wall from the last sample and steers it towards each new one by `1 − e^(−t / STEERED_OVER)` of the
  way, `t` the time since the last — a share of time, not of a frame, so a 240 Hz display steers no
  harder than a 60 Hz one — and never back past where it last stood, so the published position steers
  without its steps showing; a seek (`Seeks` moved), a new track, a pause or a
  drift past `DRIFTS_AT_MOST` takes the published position as it stands. While a synced set plays the
  pane asks every frame, drawn on the display's clock rather than the 16 ms poll's, which beats against
  it.
- **A gap breathes, mid-song as well as before the first line.** `Lyrics::waiting_at` answers only where
  no line is in play: which line the gap waits on and how far through the wait, counting from the
  track's start before the first line and from when the last line went out after it (`lyrics.md` has
  when a line goes out and why a blank line is a pause). The pane draws it as three dots standing where
  that line will be, filling in turn and swelling on `LyricsModel::breath` by `DOT_SWELLS_BY` of
  `Measures::dot`, the line itself rising towards lit as the count runs out. The dots are painted by
  `breather`, a canvas of the *fully swollen* size, each a quad around a centre that never moves, so
  the row holds still while they breathe and a dot swells by fractions of a pixel: sized to the dots,
  the gap grew and shrank and shifted every line below, and as laid-out boxes taffy rounded each dot's
  size and place to whole pixels, so a swelling dot stepped and wobbled off its centre. **The dots never change the layout: their room is the sheet's,
  not the wait's.** `Lyrics::breathes_before` answers from the timing alone which lines a wait would
  ever count down to — the first written line, and any whose previous written line goes out
  `A_BREATH_AT_LEAST` ahead of it — and every such line is drawn with `Measures::breath` of room above its
  text for the sheet's whole life, reading as the space between two verses. The dots are drawn in that
  room and only fade: a `Breathing` fades them in over `TURN` when a wait begins and a `FadingBreath`
  out over `TURN` when it ends on its line. `landing` centres a line's text rather than its row, taking
  half the room off, so lines with and without room read at one height. Adding and removing the dots as
  a child of the line made every line below jump, and easing their height let them collide with the line
  below while the rest slid apart and the glide chased the moving centre; room always there moves
  nothing. `waiting_at` lives in `resonate-lyrics` beside `LIT_AT_MOST`, so the ten seconds is one
  constant, not two. Progress *through* a line is drawn only where the set times its words (`lyrics.md`
  has the sweep), a rail under a line timed as a whole being its span pretending to be a karaoke sweep.
- **A name naming something the window can open is an `opens`, and its id decides.** `RootView::opens`
  takes an `Option<Selection>`: `Some` draws the name as a link — accent and underline under the
  pointer, the cursor, the hint and a listener selecting and showing `Pane::Tracks` — and `None` the
  same truncating element with none of them, so one call site covers a scanned and an unscanned row. It
  is the whole of how a name is pressed, every one going through it: the playback bar's title, artist
  and album, the inspector's and lyrics pane's headings, the owner under a scoped album heading, the
  artist under a grid cover, and the artist cell of every track row in the tracks, queue and playlist
  listings. The listener stops the press, a row itself being a press — clicking the artist in a track
  row opens the artist and does not also start the track, as `row_controls`' queue and playlist marks
  stop theirs. The link is `flex_shrink` under `ends_in_an_ellipsis` inside a cell, never grown, so it
  is as wide as the name — the rest of the artist column belonging to the row — and a name longer than
  the column is shrunk to it and measured again at that width, which draws its ellipsis. What a row
  knows is what `listing::Row` carries — `artist_id` beside `artist`, filled by `listing::scanned`, left
  `None` by `read` and `unread` — so a queue row the library never saw draws its artist as text.
  `unheld_row` passes `None` outright, a release track the catalog holds no row for naming an artist
  with no library id; it goes through `opens` all the same, so the cell is laid out by one function.
- **A name running past its room ends in an ellipsis, and `kit::EndsInAnEllipsis` is the one place
  knowing how.** A `truncate`d name — `overflow_hidden`, `whitespace_nowrap`, `text_ellipsis` — is cut
  where its room ends with nothing saying so: in a fixed-width cell the last letter is sliced through,
  and under the album grid's caption, a content-sized flex item, the ellipsis never came, taffy
  measuring such an item under `MaxContent` and gpui's text cache answering that measure for every later
  measure of a `nowrap` text, so the line is laid out whole and truncation never runs.
  `ends_in_an_ellipsis` is `whitespace_normal` and `line_clamp(1)`, applied after `truncate()`: a
  single-line *wrap* escapes the cache, a wrapping measure being asked with the width it has, and the
  clamp draws the ellipsis there — `truncate` supplying the overflow, nowrap and `text_ellipsis` (which
  says which glyph), the trait being where the wrapping escape is written. `opens` takes it, so every
  link this window draws ends in one — the playback bar's title, artist and album, the inspector's and
  lyrics pane's headings, every row's artist cell, the grid's caption and the Missing pane's headings —
  no call site adding it twice. **No `truncate` stands alone any more**: every other one — the
  inspector's values, a menu's entries, a sidebar tab, a pressing's line, a device's label, a settings
  row — is `truncate().ends_in_an_ellipsis()`, so a name whose room ran out because an *ancestor* did
  ends in an ellipsis rather than a sliced glyph.
  **A content-sized box does not wrap.** `kit::KeepsItsWidth::keeps_its_width` — `flex_none`,
  `max_w_full` — sets `whitespace_nowrap` back over the clamp, since taffy measures such a box at its
  min-content width and a wrapping, clamped text there drew nothing: the playback bar's title,
  artist and album went blank on every track once `opens` took the clamp. Those boxes are cut to fit
  by `kit::cut_to_fit` where they must end in an ellipsis; a `flex_shrink` cell keeps the clamp.

## Driven by tests

- **A pane is pressed, dragged and scrolled by a test, under gpui's own test platform.** `resonate-ui`
  takes gpui with `test-support` as a dev-dependency, and `driven.rs` is the harness: `Driven::open`
  sets the `ResonateApp` global as `run` does — a `Player` over an `Unplugged` backend with no sinks
  refusing every stream, an in-memory `Library`, the `Ephemeral` settings, `Places` under a temporary
  folder so a curve the pane writes lands there — and opens a real `RootView` in a `VisualTestContext`,
  activated, gpui telling a view it has focus only in an active window. It presses, right-presses,
  drags, scrolls and moves the pointer at the bounds gpui drew, read through `debug_selector`:
  `kit::Found::found_as` names every button, chip, segment, switch, choice and mark by its `ElementId`,
  and the rows, tabs, menu entries and surfaces a test reaches name themselves likewise —
  `tab-visualiser`, `track-<id>`, `queued-<id>`, `menu-entry-<n>`, `picker-<id>`, `equaliser-curve`. The
  selector is compiled out without `test-support`, costing the window nothing. `Driven::settle` runs the
  executor until it parks and advances the test clock a frame, letting the models' polls run, and
  `Driven::until` waits on the real engine and scan threads. `Driven::play` opens rows through a
  `Command::Resume`, opening the first paused without asking the graph for a sink. What is driven: a
  tab, the visualiser's Scope segment, a track's menu and an entry of it, the playlist picker, a queued
  row dragged below another, a suggestion card, the equaliser curve pressed, dragged and right-pressed,
  the settings body scrolled, and the Listen sheet with its segments and microphone chips — planted
  through `ListenModel::hearing_of`, a microphone being PipeWire's to list. `views/root.rs` drives the
  keys the same way: the queue's reach dropped from its end, a backspace after a lapsed jump, each sheet
  holding the pane's keys back, escape taking a menu before a toast, `ctrl-m` and `ctrl-u`, and two
  playlist edits asked for in one breath. **What it found**: pressing
  the curve with the equaliser off turned it on, which showed the switch's note above and moved the
  curve out from under the press shaping it, and the put-back mark appearing in a group's header grew
  the header; the note is now always there and `kit::section_header` holds a row control's height, so a
  first band lands where pressed.

## Panes

- **A favourite is a star, the heart being taken, and what is missing is neither.** `Icon::Want` and
  `Wanted` are already an outline-and-filled heart, both on a track row, so a second heart would have
  been two indistinguishable marks. The heart is the *gesture* of wanting a row and nothing else: the
  Missing pane, its empty state and an artist's *not held* button are `Icon::Missing`, the disc with its
  rim dashed away, a pane of absences drawn as a heart reading as a second favourites list; `Favourite`
  and `Favourited` are a star on the same 24-unit box at the same stroke weight. The mark hides on hover
  like every row control **unless the row is already a favourite**, the one state legible at rest — and
  everything that fades must be inside what fades: the tinted ground behind the mark on an album cell
  began as a wrapper the opacity never reached, so every grid cover wore a grey square until the ground
  moved onto the mark. The general rule: a conditional control's ground, border and padding belong
  inside the condition. **A favourite is drawn filled and in the accent** (`kit::lit_mark`) and an
  unmarked one as the grey outline every row control is, the two states differing by more than a fill
  nobody could see in grey at fourteen pixels.
- **What was favoured is what the star says from the press on, not from the next read.** The star drew
  the last catalog read, and the playback bar's and queue's rows read it from `LibraryModel::named`, a
  cache nothing re-reads — so a press wrote the favourite, the star stayed empty, and the next press,
  still reading *not a favourite*, wrote it again rather than removing it. `LibraryModel::favour` notes
  what it wrote in `favoured`, a map from `Favoured` to the answer, puts the answer into the `named` row
  where the track is cached, and notifies before the write runs; `LibraryModel::favours` is what every
  star, cell and menu asks, its own row's read the fallback, `favoured_album` and `favoured_artist`
  going through it and `favours_track` being the reading of a resolved `Track`. **The playback bar, the
  queue and an opened playlist ask it too**, their row coming from `named`, keyed by the *queue's* id: a
  resumed queue, an unscanned playlist row and a doubled row all carry an id minted apart from the
  catalog's, so `favour` patching `named` by the catalog's id missed the very row the bar drew, the star
  empty until the next run read it back. `listing::scanned` takes the answer rather than reading
  `Track::favourite`. The map lives the run and is never wrong within it, every favour this window makes
  going through `favour` — and a failing write removes its entry: `edited_then` hands its closure
  `Edited::Failed`, and `unfavour_what_did_not_save` drops the key and restores the `named` row it
  replaced before the reload, so the star falls back to what the catalog kept and the next press sends
  the opposite of what is stored.
- **Three panes were added, each the same six edits.** `Pane::Favourites` and `Pane::Suggestions` sit
  under `Section::Collection` and `Pane::Statistics` under `Section::Library`; each is a variant, a
  place in `Pane::BROWSE`, arms in `label`, `about`, `section` and `icon`, an arm in
  `RootView::content`, a sidebar count and a `mod` line. Favourites stacks the three kinds — a shelf of
  artists, a shelf of albums and the ordinary track rows — and its sidebar count is all three, a pane of
  only favourite albums otherwise reading as empty. Its order is fixed at *Favourited*, so its column
  header is unsorted: reusing the tracks pane's would light that pane's order and sort the wrong list.
- **The statistics chart is `div`s, not a canvas.** `inspector::traced` and the equaliser curve are
  continuous series where this is a few dozen discrete bars, and a `div` carries a `hint::Names` hover
  for free where a canvas needs hand-written hit-testing. A day nothing was played still draws its
  baseline, so the axis has no holes, and a run longer than `BARS_AT_MOST` (120) folds whole days into
  one bar rather than drawing a year a pixel at a time. The axis reads *today*, *yesterday* and
  otherwise the date, *29 Jan*, with the year where it is not this one, through
  `resonate-core::CivilDate` of the day `Calendar::local` says each bar's midnight and now fall on —
  the calendar the history buckets a play by, so bar and date always agree. `Chart::of` reads the local
  calendar and `Chart::in_calendar` takes one, which is how the tests hold the axis to UTC and to a
  zone east of it (`a_day_is_dated_in_the_listeners_calendar_rather_than_greenwichs`).
- **A suggestion card reads its rows off the frame.** `RootView::with_the_rows_of` is
  `with_everything_listed`'s sibling: a suggestion is a search that may name the whole library, so Play
  and Add to queue read it on the background executor before acting. *Save* is `Library::save_query`
  through `LibraryModel::edit`, so a taken name comes back as a `Notice::Trouble`, not a panic, and a
  card whose search a playlist already fills itself from greys a check under `kit::Press::Greyed` rather
  than offering a second copy.
- **The Suggestions pane is shelves by kind, and a card opens onto what it would hold.** `SuggestionKind`
  groups the cards under *From your listening*, *Eras*, *Genres*, *Artists* and *Sound*, in
  `SuggestionKind::ALL`'s order, each an eyebrow over a wrapping row. Pressing a card's art or name is
  `LibraryModel::open_suggestion`, keeping the opened `SavedQuery` in a `Previewed` and reading the first
  `PREVIEWED_AT_MOST` rows on the background executor, and `preview_further` grows that window by as many
  again whenever the list is drawn to within `LOOK_AHEAD` of full — a larger prefix of the same ordered
  read, as `reach_further` grows a browse pane's — so a whole-library suggestion scrolls to its last row.
  While the opened query is still among the offered suggestions, the pane draws it instead of the shelves:
  a way back, the art at the text column's height through `Drawn::OnThePage` (an album cover's square),
  and beside it, on the art's bottom, the kind, the name, the reason, count and length and the search as
  *Reads* chips, and under that Play, Shuffle, a save mark and a search mark, over an ordinary unsorted
  track listing, each row playing the list from itself. A shelf card is the art at full card width, still
  the grid's texture, the name under it and Play, then the queue and save marks, on one row at the card's
  bottom, so a wrapped title does not leave the row beside it short. Play leads; a labelled Add to queue
  once wrapped it onto the bottom of a card stretched to the shelf's tallest. A suggestion the catalog
  stops offering takes the pane back to the shelves. The search mark is `search_instead` and
  `choose_pane(Pane::Tracks)`, so the list can be narrowed further and saved under its own name.
  *Shuffle* loads the rows from a place picked off the clock's nanoseconds (no random crate in the tree),
  then turns the transport's shuffle on.
- **A suggestion's art is drawn, never stored, and a playlist's is the same drawing.** `views/mosaic.rs`
  is the one builder: a `Mosaic` is the covers drawn, two accents, a mark and a name, and `Framed` says
  whether it stands alone, bordered and rounded all round, or heads a card with its foot square. The
  suggestions and playlists each carried a copy once, only the suggestions' rounding the tiles, so a
  playlist's mosaic showed square corners over its rounded frame. `suggestion_art` draws the covers in
  `Suggestion::pictured_by` — one whole, or two to four as a 2×2 mosaic whose empty tiles are the ground
  tinted — over a 135° gradient between two accents of the worn palette, read through `theme::hue` so
  the art follows the theme. The accents are the reason's where it has a colour — a decade amber into
  peach, *Most played* red into peach — and a genre's or artist's are picked by an FNV hash of the name,
  so one name is the same two colours every run and two names differ. With no cover the ground carries
  the reason's icon and the name in bold, in `theme::ink_over` the first accent. A cover is asked
  through `LibraryModel::cover` at the grid's size on a card and at `Drawn::OnThePage` once opened, so
  cards fill in as decodes land. **The gradient is painted only where no cover is.** It once lay under
  the whole frame, and gpui clips a child to its parent's rectangle, not its rounding, so square covers
  stood over the rounded ground and a hairline of it showed along the frame's edge and where two
  half-pixel tiles met. The side is a whole pixel, the near tile the floor of half of it and the far
  tile the rest (`tile_span`), so far tiles reach the frame's edge on an odd side too; each tile and its
  cover are rounded on the one corner they stand in (`Corner::of_tile`), and a single cover is rounded
  itself. **A frame standing alone draws its hairline over the art, never as its own border**: a
  `border_1` on the frame took two pixels off the box the tiles lay out in, so the right and bottom
  tiles ran past it and the rectangular clip squared every corner but the top-left, a single cover
  losing its right and bottom rounding likewise. `hairline` is an absolute, rounded, bordered child laid
  last, so the art fills the whole side. On a card the bottom edge stays square, meeting the text under
  it; an empty tile is its accent, solid.
- **The playlists index is a grid of covers or a list, and the heading chooses.** `PlaylistsDrawn` is
  `Grid` (default) or `List`, a `kit::segmented` beside the sort icon kept for the run as `ArtistsDrawn`
  is. The grid is the albums pane's shape — the same measured `grid_width`, `grid_columns` and
  `grid_row` — with a card per playlist: the mosaic at `theme::grid_cover()`, the name, lit in the
  accent under the pointer or while it is the playlist in play and led by a search mark where it fills
  itself, and under it the count and when last played, or its length where never. The pin stands in the
  card's corner as a favourite's star does, hidden until hovered unless pinned, and a round accent Play
  rises in the other corner under the pointer. The list is the rows it always was, each with a context
  menu beside its controls that neither answers the other's press: gpui 0.2.2 starts a click only on a
  left press, so a right press on play, next, last, shuffle, add songs or discard opens the row's menu
  and fires none. A pinned row wears a `kit::badge` beside the `SEARCH` and `KEPT` ones; the catalog
  already sorts pinned rows first, so the pane orders nothing. `Listed::Playlists` makes either shape a
  reach-key listing, as the albums grid is: a page is whole grid rows, `show_row` scrolls the grid row
  holding the playlist, `enter` opens it. An empty index offers *New playlist* and *Import* under its
  sentence.
- **What pictures a playlist is read with the listing, never on the frame.** Each index row once read its
  playlist's whole entries from SQLite on the render thread to find its covers, discarding them on every
  library revision — every counted play, every scan poll. The load now asks `Library::playlist_pictures`
  for every listed playlist and the opened one, `PICTURED_AT_MOST` (4) covers each, and
  `LibraryModel::pictured` hands them out by reference count.
- **Pinned playlists stand in the sidebar under Playlists.** `Library::pinned_playlists` rides the same
  load, at most `PINNED_IN_THE_SIDEBAR` (5), most lately pinned first and unnarrowed by the search, and
  `RootView::pinned_rows` draws each as a short indented name under the *Playlists* row opening it — the
  open one in the text colour, the one in play in the accent. The index still lists every pinned playlist
  first, so five bounds the sidebar, not pinning.
- **An empty opened playlist says why and offers the way out.** A list offers *Add songs*, the heading's
  + track-browsing mode; a saved query matching nothing says the library holds nothing it matches yet
  and offers *Edit search*; a narrowed playlist says only that the search matched nothing in it.
  Discarding one is told in a toast naming it and `ctrl-z`, as starting one always was.
- **The sleep control draws no clock of its own.** It sits between repeat and the volume in the playback
  bar and opens a `Menu` rather than cycling — seven choices (`SLEEP_MINUTES`' five lengths, the end of
  the track, the end of the queue), eight while a timer runs and *Turn the sleep timer off* joins them —
  a toggle having to be pressed through them. The accent wash and countdown are both inside one
  `when_some` on `PlayerState::sleeping`, so an unset timer leaves a moon at shuffle's and repeat's
  weight and nothing else; the reading comes off the publication, and a part second rounds up so a timer
  just set reads `15:00`, not `14:59`. Deliberately not a setting and no config key: a sleep timer is a
  nightly decision, and a key with no control is what the settings rebuild went hunting.
- **The queue is reached from the playback bar, not the sidebar.** `Pane::BROWSE` is what the sidebar
  lists and `Pane::Settings` is pinned under it by a `justify_between`; `Pane::Queue` is reached only
  through the bar's queue button or `ctrl-u`, a `toggle` like shuffle and repeat carrying the accent that says it is
  open. It carries no count: the queue's heading says how long it is, and a figure beside the one icon
  in the cluster that had one read as a badge to clear. `RootView::behind_queue` remembers the pane the
  button covered, so a second press returns to it rather than a default.
- **The queue follows the row that started playing, only while it is the pane in front.**
  `RootView::following` is a `Following` — the `PlayingRow` last shown: the row
  `PlayerState::queue_position` names, the playing track's id and the `queue_stamp` of the queue it is
  a row of, all three off one published state — and `Following::follows` is the whole decision: the
  row, or nothing where `self.pane` is not `Pane::Queue`, nothing plays, or the playing row has not
  *moved on*. It moves on where another track plays, or where the row changed inside the same queue —
  the same track queued twice and stepped onto; a row moved only because rows above it were dropped,
  dragged or put back changes the stamp and keeps the track, so it is recorded and not scrolled to,
  where following the index threw the view away from the rows being edited.
  It rides the `cx.observe(&player, …)` `count_a_play` rides, and lifts the *NOW PLAYING* heading to the
  pane's top — `QueueParts::opens_at` is that line, and `lift_the_playing_row` scrolls to it strictly,
  gpui's plain `scroll_to_item` doing nothing for a line already in view — so what was heard is above
  the fold, reached by scrolling up. Opening the queue pane forgets the row last shown, so the pane
  always opens that way. A heading near the queue's end could not reach the top of a list ending under
  it, so `QueueParts::room_below` adds as many blank lines as the pane's measured height leaves unfilled
  below it, and the list is not built until `queue_height` is measured (the album grid's rule), so the
  first scroll is taken against the room it needs. The reach keys still centre the row they land on.
  Holding the row keeps the 200 ms poll from fighting the listener: a redraw leaving the playing row
  where it was scrolls nothing, so a queue scrolled away by hand stays until the track changes. A row
  that moved while another pane was in front is not recorded as shown, so the first poll after the queue
  returns lands on it — the list's left row is no longer the playing one, and scrolling a list nobody
  watches only yanks it later. `Following` is arithmetic over an `Option<PlayingRow>`, so
  `views/root.rs` tests it without a window as `views/reorder.rs` tests `Step::landing`.
- **The *Reads* row reads back the search box rather than claiming anything about a listing.**
  `listing::reads` is one chip per clause off `Search::reads`, standing in every pane's heading — under
  the tracks pane's, the playlists index's and an opened playlist's, and beside the albums and artists
  panes' titles, which carry their counts and nothing else. It says how the text was read, not what each
  pane did with it: `Matching::grouped` carries the whole grammar into the albums, artists and tracks
  listings and `db::cuts_matching` into an opened playlist's rows, while the playlists index narrows on
  `words_of` alone, a playlist having only a name — which its empty message says. A lone word draws no
  chip anywhere, needing no explaining. A pane with nothing left says "No albums match." where a search
  is in force and "No albums yet." over where to add a folder where none is, so the branch blaming a
  search is the branch a search caused.
- **The window keeps the queue for the next run, and the switch stopping it discards what was kept.**
  `Keeping` sits on `RootView` beside `Listening`, off the same observer of `PlayerModel`, so the window
  keeps what `resonate play` keeps by the same arithmetic (`audio.md`). `Stored::resume` is what the
  binary hands in, the flag being a `config.toml` key the window both draws and obeys, and
  `LibraryModel` holds it beside `online` and `after_scan` for that reason. The *Resuming* card is a
  Library group, not an Online one: what it turns on writes to the catalog and reaches no network.
  Turning it off is `Library::forget_resumption` at once rather than merely ceasing to write, a queue
  left behind by a setting now off coming back the next time it was turned on.
- **The window's tab, restore size and Settings category each have their own memory switch.**
  `remember-tab`, `remember-window-size` and `remember-settings-category` default on and ride into
  `ResonateApp` through `Stored`. The tab is the pane `in_front` names, so a scoped album is remembered as
  Albums; a tab hidden by `Tabs` is ignored on the next open. The size is the windowed restore size,
  written after `WINDOW_SIZE_SETTLE` so a drag edits the config once, not per compositor resize event, and
  the window opens centred at that size. The last Settings category is restored independently. Turning a
  switch off removes its saved value, so turning it on later starts from the current tab, size or
  category.
- **The window is handed its lookups and owns none of them.** `Lookups` is what `run` takes beside the
  player, the library and the settings: the `lyricists`, `fingerprinters`, `reference` (an
  `Option<Arc<dyn Reference>>`), `corrections`, the `online` setting (`enabled` and the `contact` as
  saved), `bindings`, `sourcing` and `listens`, and `ResonateApp` holds them as globals, so
  `LibraryModel::new` is built knowing whether it `can_enrich` and the settings pane knows what to draw.
  A scan not cancelled runs `enrich(false)` as it ends, where `can_enrich` — online *and* a reference —
  allows it, and the lookup's task polls at `SCAN_POLL` and reloads every `POLLS_PER_RELOAD` polls as
  the scan's does, so rows and covers arrive under the panes as the reference answers. Turning Online
  off under a run does not take the reference away; it stops `enrich` starting, and `has_reference` is
  what the card reads to say a run started offline has nothing to reach until the next start. The
  switch is heard by the shared client at once (`online.md`), and `AnalysisModel::reach` stops the
  Analysis pane asking for a recognition while it is off, `recognises` answering false.
- **A file changed under a root while the window runs is followed on its own, and one taken away is
  forgotten at once rather than scanned for.** `resonate_library::RootsWatch` is a recursive inotify
  watch over the roots, through `notify`, sorting what it hears two ways. A path *gone* — an audio file
  or folder removed, or anything but a sheet renamed away — is noted by path and handed out by
  `taken_away` once `GONE_QUIET_FOR` has passed since last heard, so a tagger deleting and rewriting a
  file in one breath is seen standing again — and never while a change under the roots is still
  settling, a rename being a path gone and a path changed at once, and the scan the change sets off
  following the file to where it went rather than forgetting it and counting it anew. A path *changed* —
  an audio file or sheet made, written or renamed in, a folder made or renamed in whatever its name
  holds (`Dr. Dre` as readily as `Meddle`), a sheet taken away — notes its root, and `settled` hands the
  root out once quiet for `ROOTS_QUIET_FOR`. An overflowed inotify queue notes every root. A catalog
  kept inside a root cannot set off the scan writing it, writing no audio or sheets there; a vault kept
  inside one does, its objects being audio, and the following scan steps past it. The model's
  `watch_the_roots` looks every `ROOTS_LOOKED_AT_EVERY` (a quarter second): it rebuilds the watch
  wherever `Library::roots` moved — `RootsWatch::taking_over` carrying every root and path the old watch
  heard and had not handed out onto the new one with when each was heard, so a drive coming or going
  costs no other root a change still settling, and a watch that cannot be laid leaving the old one
  standing and the roots untried, the next look trying again — hands what is gone to
  `Library::forget_the_gone` — taking the `Walk` guard, deleting the rows at each path or under it whose
  file is missing, sparing a vaulted and a delivered row as the scan's prune does, and sweeping what
  that orphaned — and reloads where it forgot anything; and where a root has settled and nothing holds
  the work slot it runs the ordinary incremental scan of that root, `Prompted::OnItsOwn`. A non-UTF-8
  path names nothing the catalog could have stored and is passed over rather than failing its batch. A
  forget refused because a scan or import holds the guard is kept and tried next look, as is one the
  store failed, so a file deleted mid-scan leaves the list the moment the scan ends rather than after
  another quiet period and whole-root scan. It replaced waiting for the root to be quiet then rescanning
  it whole for every deletion, so deleting files one after another kept pushing the scan back and a
  large root took its whole walk to drop one row. A watch that cannot be made — inotify out of watches —
  is a warning, and that root waits for a scan by hand.
- **What changed while the window was closed is caught up the moment it opens, and a missing root is
  watched once it appears.** A watch hears only what happens while it stands, so a file added between
  runs waited for a scan by hand. The first look that has read the roots runs one incremental scan of
  every root, `Prompted::OnItsOwn` — asked with no roots named, so an unmounted root is left alone by
  `is_there` rather than failing the scan with `RootNotADirectory` and holding back the rest — waiting
  for the work slot like any owed root, a lookup the last run left unfinished not holding it. `Watching`
  watches only roots that are folders now, keeping the rest as `absent`; one turning up later — a drive
  mounted after the window opened — rebuilds the watch and is owed an incremental scan of its own, where
  once it was read once and never watched.
- **A browse pane holds a window onto the listing, and every count comes from the catalog, not the
  window.** `LibraryModel::reach` is how many rows `browsed` asks for — `PAGE` (2 000) to begin — and
  `reach_further` grows it by a page when a list has drawn to within `LOOK_AHEAD` of what it holds *and*
  holds all it asked, which says there may be more. Growing re-reads with a larger limit rather than
  appending a page at an offset: the sort is redone either way, and a window that is always a prefix of
  one ordered read cannot duplicate or skip a row as a second offset read can under a scan. It is called
  from inside the `uniform_list` processors, the one place knowing how far a list has been drawn; `held
  < reach` is the re-entrancy guard, a growth in flight leaving the window larger than what is loaded
  until it lands. `LOOK_AHEAD < PAGE` is a `const` assertion, not a hope: a look-ahead past the page
  reaches the *first* window's end on the first frame and the window grows itself to the whole library
  unasked. `set_query` and `select` put it back to one page, the rows under the old window not being
  those a new query names.
- **What a count says is what the query matches, not what the window holds.** `Library::albums_counted`,
  `artists_counted` and `measured` answer over the whole match — the first two sharing their scoping
  with the listings through `narrowed_onto`, so a count and a listing cannot disagree about what the
  search means — and `Measured` carries the rows, the total length and how many are lossless, the last
  from a `tracks.codec IN (…)` built off `Codec::ALL` filtered by `is_lossless`, so SQL and Rust cannot
  drift. The sidebar's three figures and the tracks heading's summary read those rather than `Vec::len`,
  which reported 2 000 for any larger library. `measured` still honours a limit it is given, a saved
  query's cap being part of what it *is*; the window passes `limit: None`.
- **A gesture over "every track listed here" reads the whole listing before acting.**
  `RootView::with_everything_listed` reads the unlimited `TrackQuery` `listing_whole` builds on the
  background executor and hands the rows to a closure, so *Play* and *Shuffle* in the tracks heading act
  on everything the search matches, not the loaded window. Whole-listing queue and add-to-playlist
  actions are not in that heading; track rows still expose their queue controls on hover. A task on
  `RootView`, not a blocking read, the listing possibly being the whole library.
- **A scope is the tracks pane's alone, and the sidebar counts the library, not the scope.** `Selection`
  narrows one listing and no other: `browsed` reads the albums, artists and tracks the search allows,
  and a *second*, narrower track listing beside them only where an album or artist is scoped.
  `LibraryModel::tracks` is the first and `LibraryModel::listing` what the tracks pane, its heading, its
  summary and its *Play* and *Shuffle* draw from, so the sidebar's three counts say what the library
  holds however deep a listener has gone. It replaced one listing narrowed in place: opening an artist
  took its albums out of the albums pane and its count down to that artist's, opening an album took the
  tracks count down to that album's, and nothing said why. The second read is the cost, and the cheap
  one — an album's dozen rows, not the library's two thousand — which is why the whole listing is the
  one always read.
- **A scoped heading names its artist on a line of its own, and its cover magnifies.** The album hero
  draws the owner between the title and the summary rather than folded into the summary's first field, a
  name run together with *1973 · 10 tracks · 43:12* being unpressable at only its own width; it goes
  through `RootView::opens` like the playback bar's, gesture and hint written once. The scoped cover
  carries `COVER_HINT` and `RootView::magnify`, so the album being read is as magnifiable as the one
  playing. A portrait is not: `Magnified` is an album or a file, and a portrait is neither.
- **An album's rows are what the catalog holds merged with what the release says is missing.**
  `LibraryModel::rows` is an `Arc<[ListedRow]>` built by `album_rows` whenever the selection is an
  album: `ListedRow::Held` indexes the tracks listing and `ListedRow::Missing` the release rows whose
  `track` is `None` — beside the variants of their own, `Disc` heading each disc's run and `NotHeld`
  and `Found` below an artist page's rows — the two sorted together by `(disc, position)` — a held
  track at its paired release row's place, or its own disc and number where nothing paired it, no number
  sorting last — so the tracks pane's `uniform_list` counts `rows` inside an album and `tracks`
  everywhere else. That seat order is drawn while the pane's sort is the album's own, `Relevance` or
  `AlbumThenTrack`, turned round where it reads backwards; any other sort the heading's sort icon picks
  draws the held rows in that sort and what the album lacks after them (`arranged`). **What plays is
  what is drawn**: a row's click and Enter go through `LibraryModel::played_from`, queueing the `Held`
  rows in `rows` order, and *Play*, *Play next*, *Add to queue* and *Add to playlist* put the whole
  listing they read through `AsDrawn::ordered`, the same arrangement over the same release rows — so the
  row under the pointer starts and what follows is the rows under it on screen. **A missing row on an
  album page is a download press.** `Beside::AnAlbum` makes an unheld release row clickable when it
  has no want, with the title, artist or length click asking `LibraryModel::want`; marks and dismissal
  controls stop propagation so they keep their own action. When the album heading counts any missing
  tracks, *Get the rest* calls `LibraryModel::want_missing_tracks`, which wants every unresolved
  release row in one library transaction and starts one provider poll for the batch. `unheld_row`
  draws faint in the same eight cells, number, title, artist and length off an `Unheld` — built `From` a
  `HeldReleaseTrack` here and `From` a `MissingTrack` in the Missing pane, one row serving both — and in
  `controls_place_of`, the width of the held row's controls beside it — `TRACK_CONTROLS`, `TRACK_ADD_CONTROLS` while songs are being added, so a missing row's cells line up with the held rows' and the header's; the Missing pane's `ROW_CONTROLS` — one mark: `Icon::Want` sending `LibraryModel::want` or
  `Icon::Wanted` sending `unwant`, both through `edit` like a playlist gesture — `want` through
  `edited_then`, asking the providers once the want is written — with `Loaded::wanted` mapping each
  `ReleaseTrackId` to its `WantId` so the mark knows which it is. **A want says how it is going where
  it was pressed**: `LibraryModel` keeps each want's `WantStanding` from the shelves and `fetching_want`
  reads it as a `Fetching` — *Queued*, *Downloading* while the poll asks for that want, *Retrying*,
  *Gave up*, *Downloaded* — and the row's format cell says it in `fetching_colour`, so a filled heart is
  never the only answer. `want` and `want_missing_tracks` also list each row they want, one with a recording, in `Downloads` as a `Found` built off the album page's release rows (`list_as_downloads`), so the sidebar and the downloads panel show it with the attempts as a found song's are shown; both start the poll through
  `fetch_or_say_nobody_can`, which toasts *No provider is set up* where `has_a_source` is false, as a
  found song's caption does. Neither the grid's caption nor the
  scoped heading counts what an album is short of any more: the Missing pane, the inline `unheld_row`s,
  the sidebar figure and the artist heading's *N releases not held* each say it once where it is the
  subject, not on every cell. The pressing is not drawn there. An info mark at the end of the row under
  the text, only where `record_of` has something to say, opens a card anchored at the press: Released,
  Format, Label, Catalogue number, Barcode, Country and Kind, then the disambiguation, then every
  service link. Where the album carries a release group and the build can reach the reference, *Other
  pressings* asks `Reference::release_group` on the background executor —
  `LibraryModel::ask_for_pressings`, the answer held against the album in `pressings` — and lists the
  group's releases in release order, the first `PRESSINGS_SHOWN`, each as date, country and track count,
  the one in use badged *IN USE*; a press on another is `LibraryModel::take_pressing`, landing it
  through `Library::take_pressing` and closing the card. **A track is placed on a release from its own
  menu**: *Place on a release…*, offered wherever the reference can be reached, reads the track's
  recording through `LibraryModel::releases_it_could_sit_on` and opens a second menu where the first
  stood, the releases listed as a found song's are — title, year and kind, `RELEASES_OFFERED` of them —
  a press being `LibraryModel::place_on`; a track nothing identified says so as a toast. **An album
  nothing matched has the card too**, wherever the reference can be reached: the mark is drawn however
  little `record_of` says, and the card offers *Find the record* in place of *Other pressings* —
  `LibraryModel::ask_for_releases` asks `Reference::find_release` in words with the album's title,
  artist and any barcode or catalogue number the tags gave, listing what comes back as a group's
  pressings are, each named with its credit, so a press is the same `take_pressing` and an album a
  lookup could not settle is settled by the listener. Where the album was matched the card ends in *Not
  this record*, taking two presses — the second under *Press again to forget the match*, the arming held
  on `OpenedRecord::Album` so any way the card closes lowers it — and is
  `LibraryModel::forget_the_match`, told as a toast. Escape closes it after a magnified cover and before
  a toast, a second press on the mark closes it, and leaving the album (`set_pane`) closes it. The scrim
  occludes, as a menu's does. An artist hero draws `profile_line` and `heard_on`, its genres opening
  from the info mark — every one, as `kit::tag` pills, the mark drawn only where there is one. At the
  end of its actions, where `ArtistDetail::releases_unheld` is above nothing, a `Tone::Ghost` button
  reading *N releases not held* under `UNHELD_HINT` opens `Pane::Missing`. **An artist's services are
  capped at `SERVICES_SHOWN` (4), an artist having a directory of them.** A release's card is not, nor
  the genre card: the cap was for a heading line, and a card has room. An artist named nineteen — every
  streaming service, both encyclopaedias and four social networks. Four is what the heading draws, as
  one line of words rather than a cloud of mono figures. **Each service on the line is the link it was
  named from.** `service_names` keeps the first URL a service is linked by beside its `Service::title` —
  *Apple Music*, *SoundCloud*, not the stored lowercase key — leaving out `Service::Other`. `heard_on`
  and the record card draw one pressable name per service, lit under the pointer as an `opens` is,
  reading *Open on …* and handing that release's or artist's exact URL to `cx.open_url`, the desktop's
  browser. It stops its press and ignores a right one, like every heading link.
- **The artists pane is a list or a grid, and the heading chooses.** `ArtistsDrawn` is `List` — the rows
  it always was, a small portrait beside each name — or `Grid`, the albums pane's shape with portraits
  in place of sleeves: `artist_grid` measures the same `grid_width`, reads `grid_columns` and lays out
  rows of `artist_cell`s at `theme::grid_cover()`, each a round `portrait_frame` read at
  `Portrayed::InAGrid` — or `kit::avatar_at` at that size, the initial scaled with it, where no portrait
  is held — over the name and its counts, favoured, pressed and menued as a row is. The choice is a
  `kit::segmented` of *List* and *Grid* beside the sort icon, living for the run like the listings'
  orders. The reach keys serve both: in the grid a page is whole rows of cells, `show_row` scrolls the
  grid row holding the artist and a reached cell wears the album's `reached_ring`.
- **An artist's page is its albums or its tracks, one at a time, chosen from two tabs.** It stacked a
  horizontally scrolling strip of small covers over the whole track listing — three scrolling regions,
  an album a thumbnail, every row repeating the artist's name. `artist_tabs` is a `kit::segmented` of
  *Albums* and *Tracks*, each with its count, at the right of the action row rather than in a band under
  the hero — that band being the gap between title and listing — and `RootView::artist_shows` is the
  `ArtistShows` chosen, kept for the run and reset to `Records` whenever `RootView::opened` opens an
  artist. The heading keeps its bottom padding, so the rule under the title does not sit on the buttons.
  *Albums* is `artist_records`: every album the artist owns or plays on as a wrapping grid of
  `album_cell_captioned` cells at `theme::grid_cover()`, scrolling under an id keyed by the artist so
  the next artist opens at its top. `Caption::Beside` captions them — year and track count, and the
  owner only where somebody else, what an album the artist merely plays on must say. *Tracks* is the
  ordinary listing under the ordinary header, the only tab offering the sort icon, a sort having nothing
  to order under the other. `ArtistShows::within` answers *Tracks* for an artist holding no album, held
  or not, and then no tabs are drawn. *Play*, *Add to queue*, *Play next* and *Add to playlist* read the
  whole listing either way, acting on the artist, not the tab.
- **What an artist's discography holds that the library does not is on the artist's page, greyed.**
  It rides in the load beside the artist's albums — `Library::albums_not_held_by` and
  `songs_not_held_by` (`library.md`) — whether the Missing tab is shown or not, the page being where the
  listener looks for an artist's work. *Albums* follows the held cells with `not_held_heading`, *Not in
  your library · N releases*, a full-width line breaking the grid, then an `album_not_held_cell` per
  release: the pressing's front through `released_cover_with_group` at `UNHELD_COVER` — read from `Library::unheld_cover`, which the lookup pass fills (`library.md`), before the archive is asked live, and drawn offline from what is kept — with the
  release-group id as its fallback and as the lookup where no pressing is known, or `unheld_cover` at
  the grid's side where neither answers, the title in `theme::faint()` and kind and year under it.
  Pressing
  one is `LibraryModel::want_album`, which runs `Library::want_album` on the background executor, hands
  every song wanted to `Downloads` as a found song pressed would be and sends the providers for them;
  `fetching_album` reads the album's songs back out of `Downloads` — the active attempt where any is,
  otherwise the first still underway, otherwise the first not downloaded — and the caption says it in
  `fetching_colour` while the cell takes no press. An album landed with nothing on disk yet stays in the
  grid, so it does not vanish the moment it is pressed. *Tracks* follows the held rows with
  `ListedRow::NotHeld`, *Not in your library · N songs · press one to download it*, and a found row
  per song — the same `found_row` a search draws, pressing it `want_found` — `restate_the_listing`
  building the rows through `beyond_the_listing`, which answers nothing until the listing is whole, so
  a paged listing never draws later pages under the section. A query on the page narrows them through
  `still_answering`.
- **The Missing pane is what the catalog knows it is short of, headed by run, one half at a time.**
  `Pane::Missing` sits under `Section::Collection` beside the playlists, its sidebar count
  `Missing::tracks`, hidden at zero, so a library the reference never described carries no figure for
  nothing. The two halves are two lists chosen from two tabs, as an artist's page chooses:
  `missing_tabs` is a `kit::segmented` of *Tracks* and *Releases*, each with its count, and
  `RootView::missing_shows` the `MissingShows` chosen, kept for the run. `MissingShows::within` hands
  over to the half holding something where the chosen one holds nothing, the tabs drawn only where both
  do; the artist heading's *N releases not held* opens the pane on *Releases*. They once ran on in one
  list, where an artist's heading after the last album's rows was indistinguishable from another
  album's. Each tab is one `uniform_list` over `LibraryModel::missing_track_rows` or
  `unheld_release_rows`, `Arc<[MissingRow]>`s of `Album`, `Disc` and `Track`, or `Artist` and `Release`,
  indices `models::missing_track_rows` and `unheld_release_rows` build: a heading wherever the key
  changes and a row per entry (`each_run_of_an_albums_missing_tracks_is_headed_by_the_album_once`,
  `each_run_of_an_artists_unheld_releases_is_headed_by_the_artist_once`). The artists' half is
  `headed_by_run` over one key; the albums' half `headed_by_album_and_disc` over album *and* disc, a
  set's missing rows numbering from one again under each disc and one heading over the lot reading as
  one album with two track ones. A disc heading is drawn only where the album's own run spans more than
  one disc (`headed_by_disc`'s rule inside an album;
  `an_album_missing_rows_from_two_discs_is_headed_by_each_of_them`), an eyebrow reading *DISC N* under
  the title column and no more, the pane holding no `HeldMedium` to name a format or title with. **A run
  is a card, drawn a row at a time, every `uniform_list` row being one height.** `Place::of` reads where
  a row stands in its run — the heading `Head`, the row before the next heading or the end `Last`,
  everything between `Within` — and `in_a_card` draws that slice of a `kit::section`-like card: the
  heading on the `raised` ground with the card's top edge and rounded top corners, the rows on `surface`
  with a hairline under each, the last closing the card with rounded bottom corners. The gap between
  cards cannot be a margin, so it is taken from the rows bordering it: the heading and the last row are
  each `HALF_BETWEEN_CARDS` shorter than their row, the heading at its row's foot and the last row at
  its head (`a_run_is_one_card_opened_by_its_heading_and_closed_by_its_last_row`). Rows laid loose under
  a heading band of their own read as text floating with nothing holding it. `run_band` is what the
  heading holds: the album's cover or the artist's portrait — `kit::avatar` where none — in the number
  column, the name through `opens` in the semibold text colour, the owner beside it in the muted one,
  and how many the run holds — *7 missing*, *11 releases* — ending where the lengths end. A track row is
  `unheld_row` under `Beside::ARun`, dropping the format and plays cells, the pane having no header for
  them; a release row is `release_row`, its title muted, its kind a `kit::badge` and its first release
  year in the length column, with no press, a release the catalog holds nothing of naming nothing it can
  show. The subtitle speaks for the tab in front — *8 tracks missing from 2 albums*, *46 releases by 8
  artists not held*. `Loaded` reads `missing_tracks` and `unheld_releases` under `MISSING_AT_MOST` (5
  000), and `missing_counted` on every load, so the tracks and releases the subtitle and tabs count are
  the catalog's, the albums and artists beside them being the headings the list drew. **A list cut short
  of its count says so**: where the tab lists fewer rows than the catalog counts, `listed_short` writes
  *5000 of 7214 tracks listed; type to narrow to the rest* under the subtitle, a count of every missing
  track above 5 000 rows having read as a list that ended. **A row is dismissed with its ✕.** A track
  row carries `Icon::Close` beside its want mark under `Beside::ARun` alone — an album page's missing
  row and a search's do not — sending `LibraryModel::dismiss_missing`, and a release row carries one
  sending `dismiss_release`, both through `edit` as a want is; the heading's *Bring back N dismissed*,
  drawn while `Library::dismissed` counts any, is `bring_back_dismissed`, told as a toast. `library.md`
  has what a dismissal is. Empty, it is
  `kit::empty` under `Icon::Missing` saying nothing is missing, and where the build `can_enrich` a
  second sentence says where the answer would come from.
- **A song asked for is followed in the sidebar, never told in a toast.** `downloads.rs` is the
  whole of it: `Downloads` holds a `Download` per found song pressed — the `Found`, the `WantId` once
  landed, when it was queued and a `Fetching` — and `LibraryModel::want_found` writes it rather than
  telling a toast: `Landing` while the release lands, then `Queued` (or `NoProvider` where
  `Sourcing::providers` registers none, `Unwanted` where the landing failed). `Download::fetching_while`
  reads a queued download as `Downloading` while `PollProgress::asking` names its want, and
  `Downloads::followed` reads each shelves load's `WantStanding`s — a want delivered (`held` or
  `offered`) is `Downloaded`, one tried since it was queued with nothing offered
  `Retrying { tries, at }` — *No match on attempt 2 of 6 · trying again at 14:32*, the clock time
  (`format::time_of_day`, the listener's zone) rather than a countdown so an idle window does not
  draw a stale one — and one the catalog gave up on `GaveUp`, *No match after 6 attempts*. A queued
  download says *Queued · attempt 1 of 6*; while the poll asks, `Downloading { attempt }` says
  *Attempt N of 6 · asking providers…*, retaining the next attempt number across retries. A poll that
  ended with a provider refusing or running late (`PollStats::refused` or `late`) sets
  `LibraryModel::providers_unheard`, and while it holds a queued download, or a retrying one whose
  clock time has passed, reads `Unreached { attempt }` — *Attempt N of 6 · a provider didn't answer,
  asking again shortly* — since such a want is never stamped (`providers.md`) and would otherwise
  alternate between *Downloading* while asked and a stale *No match* between polls; a poll that
  hears every provider clears it, a cancelled one leaves it (`Fetching::while_polling` is the one
  reading, shared by the song list and the album's cells). All of them count as underway. A want the
  load does not hold is left as it stood, a stale load being no evidence it went. The sidebar draws
  `RootView::download_status` above the enrichment line while the list holds anything — the
  `Icon::Download` in the accent while anything is underway; the label shows the active attempt
  number while the poll asks, *Queued* or *Retrying* while it waits, *Adding to the catalog…* while the
  release lands and *Downloads* in `faint` once all is finished — and a press opens
  `downloads_over_the_app`, a panel floating
  `DOWNLOADS_PANEL_GAP` beside the sidebar and above the playback bar (`theme::downloads_width`, its list
  `theme::downloads_height` at most and scrolled past that), since the sidebar of a short window had
  room for one song and a half: each song's title, its state in `browser::fetching_colour` (accent
  downloading, `done` downloaded, `failure` given up, unprovided or unwanted, `muted` landing, queued
  or retrying) and its artist, an `Icon::Redo` asking again where `can_be_asked_again` — a retry,
  which it tries at once, or a song given up, which it starts again from the first try — a ✕ on a
  finished one, and
  *Clear finished* and a close mark in its heading. A song that can be cancelled
  (`Fetching::can_be_cancelled`: queued, downloading, unreached or retrying — not landing, which has
  no want yet) has an `Icon::Stop` beside the redo: `LibraryModel::cancel_download` takes it off the
  list, withdraws its want (`Library::unwant`) and, where the poll is asking for that very want,
  cancels the poll, whose other wants stay due and are asked again by the next one
  (`pressing_an_ncs_song_found_on_musicbrainz_wants_it_and_asks_the_providers_for_it` presses it). A second press on the row, the close mark or
  escape — after a menu, before a toast — puts it away; it is not modal and holds no key back. Asking again is `want_found` once more, which makes the want due at once in the catalog
  (`library.md`), so the ordinary poll asks for it whatever the list remembers — once relied on the
  list holding the earlier failure, a song asked for again after *Clear finished* or a restart sat
  *Queued* for six hours. A self-started
  poll joining while the list holds anything tells no toast. The found row says the same thing
  where it stands: its format column draws the `Fetching` in its colour in place of the release
  title, the want mark is greyed with the state as its hint, and the row is a press only where
  nothing is asked for yet or `can_be_asked_again`, so the song stays in the results showing how it
  is getting on rather than vanishing as the search is asked again. The section heading says *Press
  a song to download it*.
  `pressing_an_ncs_song_found_on_musicbrainz_wants_it_and_asks_the_providers_for_it` asks for a song a
  shop does not have, waits for its first retry, opens the list and holds no toast up; the three tests in
  `downloads.rs` hold the states.
- **A song in the queue is taken out from its menu wherever it is listed.** A track row's menu —
  the tracks pane, a search, an album or artist page, the favourites — offers *Take out of the queue*
  right under the queue entries while `RootView::is_in_the_queue` finds a queue row of the same
  location and span; a press is `take_out_of_the_queue`, dropping every such row from the last up
  through `drop_rows`, so the put-back toast follows as it does in the queue pane, whose rows keep
  their ✕, menu entry and delete key
  (`a_queued_track_is_taken_out_of_the_queue_from_its_menu_wherever_it_is_listed`).
- **A song is deleted from disk only through a dialogue saying it cannot be undone.** A track row's
  menu offers *Delete from disk…* (`Icon::Delete`), which is `RootView::ask_to_delete` with a
  `Deleting` — the track, its title and artist and whether it is a cut. `deletion_sheet` is a card over
  the scrim as the Listen sheet is (`theme::confirm_width`), naming the song, saying the file goes from
  disk and the plays with it, adding that a cut takes its whole file and every cut from it, and that it
  cannot be undone; *Cancel* (`Tone::Outlined`), escape and a press outside keep the song, and only
  *Delete* (`Tone::Destructive`, the failure colour on the raised fill — the fourth tone, kept for a
  gesture losing something for good) calls `LibraryModel::delete_track`, which runs
  `Library::delete_tracks` on the edit chain and toasts *Deleted “…”*, or trouble where the file stayed.
  No key confirms it, `enter` included. While open it `something_stands_over_the_pane`
  (`the_delete_sheet_holds_the_keys_back_from_the_queue_behind`), and
  `deleting_a_track_asks_first_and_takes_its_file_only_once_confirmed` presses *Cancel* then *Delete*.
- **A search is a page of its own, and what the library lacks is never buried under what it holds.**
  `views/search.rs` is the whole of it. `RootView::search_in_front` answers a `SearchShows` wherever
  the box holds words, the pane in front is Albums, Artists or Tracks, nothing is scoped and no songs
  are being added to a playlist; `content` then draws `search_pane` in place of the pane. Its heading
  (`search_heading`) is *SEARCH* over the words in quotes, a summary — *12 songs · 3 albums · 1 artist
  in your library · 18 songs not in it* — the *Sung in*, *Save this search*, sort, *Shuffle* and
  *Play* actions, the *Reads* chips, and a row of tabs, each its count in a pill: *Top results*,
  *Songs*, *Albums*, *Artists* and *Not in your library*, the last only where the build `can_enrich`
  or something was found, counting *…* while MusicBrainz is asked. *Songs*, *Albums* and *Artists*
  are the three panes themselves — `tracks`, `albums` and `artists` take `search_heading` in place of
  their own — so choosing one is `show_in_the_search`, which sets the pane under it and keeps every
  reach, sort and scroll the pane already had; a sidebar press on one of the three while searching is
  that tab (`SearchShows::in_place_of`). *Top results* (`top_results`) is one scrolling page: the
  matching artists as a strip of `ARTIST_AT_THE_TOP` portraits, the first `SONGS_AT_THE_TOP` (5) songs,
  the first `FOUND_AT_THE_TOP` (6) songs not in the library with what MusicBrainz is doing beside the
  heading, then the albums as a strip — each section headed by its name and, where more matched than
  it shows, *See all N*, which opens its tab. *Not in your library* (`not_in_the_library`) is every
  found row in a `uniform_list` of its own. A search begins on *Top results* from the tracks pane and on
  the pane's own tab from Albums or Artists (`SearchShows::opening_on`, read where the box goes from
  empty to holding words). It replaced the tracks pane listing every held row and only then, once the
  whole listing had been paged in, the songs MusicBrainz found — searching an artist with a hundred
  songs held put the found ones a hundred rows down, out of sight. So under *All tracks*
  `LibraryModel::rows` is empty — one row per track, what `played_from` and `listed_rows` read — and
  `LibraryModel::elsewhere` answers the MusicBrainz half on its own as a `Beyond`: `Elsewhere(n)`,
  `Refining(n)` while asked again, `Asking` and `Unreached`, through `elsewhere_standing`. The keyboard
  follows: `reachable` reaches the top songs under *Top results* (or the found rows where none is
  held) and `Listed::Found` under *Not in your library*, enter on a found row being `want_found`.
  `songs_not_in_the_library_stand_on_the_first_page_however_many_it_holds` scans forty held songs and
  asserts the found row is drawn inside the window. A search does not list the rows a held album is
  short of — those stay in the Missing pane and on the artist's page — only the songs of releases not
  held at all, which `songs_kept_for` answers. A found song's
  `unheld_row` has a `Beside::ASearch` cover column for its release's front, `Sleeve::Released`, which
  `LibraryModel::released_cover` asks the reference for on the background executor while Online is on —
  `FETCHES_AT_ONCE` (2) at a time, decoded on the `Drawer` and held under the release's id in
  `released_covers` (`RELEASED_COVERS_HELD`, 512 — more than a discography draws, or the grid evicts what it is still asking for) for the run, a release the archive holds nothing for held as nothing so it is not
  asked again — the matched runs lit,
  the release in the format column — and the want mark is `want_mark` over an `Asks`: a catalog row
  wants its `ReleaseTrackId` as ever; a found song calls `LibraryModel::want_found`, landing its release
  and wanting the row on the background executor, greying the mark meanwhile, then asking the providers
  and MusicBrainz again, so the song leaves the remote results and appears among held tracks after
  delivery. **A found song's whole row is that press**: `found_row` lays the row `unheld_row` draws
  under an id of its recording with the pointer, a hover wash and `FETCH_FOUND_HINT`, so pressing anywhere on it wants
  the song and sends the providers for it, the mark's own press inside it finding the recording
  already `wanting` and doing nothing twice; while it is asked for the row takes no press
  (`pressing_an_ncs_song_found_on_musicbrainz_wants_it_and_asks_the_providers_for_it`, which opens the
  window through `Driven::reaching` — a `Reaching` naming the reference and the providers). What
  becomes of it is the sidebar's downloads list, above. `wanting` holds a task
  per recording, not one for the lot, so wanting a second song before the first landed does not drop the
  first's lookup and leave its mark grey. **A search reaches MusicBrainz as
  seldom as it can and never leaves the listener looking at nothing.** MusicBrainz answers one request
  a second, so the window spends its turns only on what is still being typed:
  - *Asked once per spelling.* An answer is kept in `answers`, a `Recent` of `ANSWERS_HELD` (128)
    keyed by `songs_asked` — the words as sent, so case, spacing and grammar the ask ignores share
    one — holding the reference's matches rather than the `Found`s, which `Library::unheld_among`
    weighs again on the background executor each time one is shown, so a song that landed since
    leaves the list. Typing back to a remembered search shows it at once, no settle and no request
    (`words_searched_again_are_answered_from_memory_rather_than_asked_twice`).
  - *One ask in flight, the latest text winning.* `ask_elsewhere_after` waits
    `ASKED_ELSEWHERE_AFTER` (450 ms) behind the keystroke, only where the build `can_enrich`, Online
    is on and `songs_asked` names words worth it, then `reach_out`. While one request is out
    (`reaching_out`) another is not queued behind it: the text is `owed`, and when the request comes
    back its answer is remembered whatever it was for and the text still in the box is asked next —
    so a run of pauses mid-word costs at most two turns, not one per pause, and an answer outrun by
    the box is never drawn over what it now says. Enter in the search box (`ask_elsewhere_now`) skips
    the settle.
  - *Narrowed while asking.* From the keystroke until the answer lands, `found` stands in as
    `narrowed` — the songs the last answer found that `still_answering` the new words — under
    `Beyond::Refining`, the summary reading *N songs not in it so far, asking MusicBrainz for the
    rest…* and the tab counting *N…*; only where none still answers does `Beyond::Asking` stand alone,
    *Asking MusicBrainz…* where the found songs would be
    (`songs_found_for_fewer_words_stay_listed_while_more_are_asked_for`).
  - *A failure says so.* A request refused or unreachable for the text in the box is not
    remembered; its text is `unreached_for` and `elsewhere` answers `Beyond::Unreached`, *MusicBrainz
    could not be reached* with *Try again* (`ask_elsewhere_again`) beside it
    (`a_search_musicbrainz_refused_says_so_and_is_asked_again_on_a_press`). A failure for a text typed
    past is a debug line and nothing more. Turning Online on asks for what the box holds; turning it
    off drops the ask.
  The answer drawn is kept against the text it answered (`found_for`), so a reload does not ask again.
  - *What the catalog already knows comes first.* `Library::songs_kept_for` — the songs of every
    library artist's releases not held, learnt in the lookup pass (`library.md`) — rides in the load
    with the text it was read for (`kept_for`), so a song by an artist the listener has is listed the
    moment the listing is, offline too, before MusicBrainz is asked; `kept_before_the_rest` puts them
    first and drops a MusicBrainz answer naming the same recording or the same folded title and credit
    (`songs_kept_from_a_discography_come_first_and_musicbrainz_does_not_repeat_them`).
    `restate_what_was_found` builds `shown`, what `found()` answers and the rows count, from both.
  A search whose plain words a held track sings is offered as `lyrics:"…"`: `Library::sung` rides in
  the load, and *Sung in N tracks* stands in the heading's actions, and in a pane's empty state where
  nothing else matched, as a `search_instead`.
- **What a search was read as is said under its words, and the words as typed are a press away.**
  Where `LibraryModel::meant` holds a `Meant` (`library.md`) the search heading draws *Read as
  “You F O” by Stela Cole* in the accent under the quoted words, the *Reads* chips show the scoped
  search it ran, the matched runs light by it, *Save this search* saves it, and *Search the words as
  typed* is `search_as_typed`, which reads them literally until the box changes
  (`a_title_by_an_artist_is_searched_as_that_title_by_that_artist`).
- **A link to a song pasted anywhere is the song, downloaded, not words to search.** The search
  field is built `catching(is_a_song_link)`: a paste the predicate takes is emitted as `Caught` rather
  than inserted, and `ctrl-v` away from any field is `PasteAway`, a paste into the search box, so the
  same catch answers both. `RootView::follow_link` reads the `SongLink`, asks
  `Library::follow_link` on the background executor through `LibraryModel::follows_links` — the
  reference while Online is on, else a toast saying to turn it on — and `followed` answers: a song a
  file already holds says so in a toast; a song found is `want_found`, the downloads list in the
  sidebar following it as a pressed found row would be. The box is left as it was — the link never
  lands in it and nothing is searched for — so a paste starts the download and nothing else. A link
  nothing names is a toast.
  `a_song_link_pasted_into_the_search_is_downloaded_and_leaves_the_box_as_it_was` and
  `pasting_at_the_window_puts_the_words_in_the_search` are the claims; `library.md` has which links are
  read and `online.md` how a service is asked.
- **The search box narrows the Missing pane, the two halves answering differently, one being in the
  catalog and the other not.** Both reads take the browse panes' query, so the *Reads* row stands under
  this heading too and counts, sidebar figure and list cannot disagree (`library.md` has the SQL). A
  missing track is narrowed by the *album* it is short from, through the catalog's grammar, so typing an
  album, its owner or a held track brings up what it lacks. An unheld release has only its own row, so
  it is narrowed by the fold of its title, kind and first release date, or by its artist — narrowing a
  prolific artist's discography by year and kind with no control per field. Narrowed to nothing the pane
  says *Nothing missing matches* rather than that nothing is missing, and offers no lookup, the browse
  panes' branch.
- **The playlists pane is an index and one opened playlist, not a `Selection`.** `Selection` names
  Everything, an album or an artist; a playlist is `LibraryModel::opened`, its entries riding in the
  same `Loaded` snapshot as albums, artists, tracks and roots — so nothing else needs to know when a
  playlist changes. Rows are put in one through `RootView::hold_for_a_playlist`, the picker a queue
  row's + and the queue's *Save as a playlist* open. The index's and opened playlist's + enter
  track-browsing mode for the target; each track row's + adds that track directly, and *Done* or Escape
  returns to the playlist. The picker offers a second listing riding in that snapshot —
  `Library::playlist_lists`, read unnarrowed and holding only list playlists: unnarrowed because the
  gesture is most often made from a search and a track found by name must still reach a playlist whose
  name has nothing to do with it; lists alone because a saved query refuses rows anyway, taking the
  `count(*)` per saved query off the read. Riding in the snapshot leaves the picker nothing to read on
  the frame it opens and puts a playlist started while it is open on it, so `adding` carries the `Held`
  alone. `Offered` is the arithmetic over it: the playlist the rows came out of is stepped over by
  position rather than filtered, so a `uniform_list` draws the rows — `theme::PICKER_ROWS` at a time —
  with no copy of the listing per frame. `RootView::show_playlist` opens one, taking the
  `UniformListScrollHandle` `left_at` holds against that playlist's id, so returning to a long one lands
  where it was read to. `set_query` puts in a fresh handle whenever a keystroke narrows the rows
  differently, the rows under the old offset no longer being those it was left on, and
  `forget_what_has_gone` drops the handle once the listing stops naming its playlist — passing over a
  search-narrowed listing, what that leaves out being still there. Index cards and rows use the opened
  hero's mosaic; their + starts the target's track-browsing mode rather than copying it. The index puts
  its sort icon with the icon-only Import and New playlist in the heading's right actions; pressing it
  reveals the ORDER and READS choices under the heading, and pressing again tucks them away. None of it
  survives the run, nor does the settings pane's category, the listing's order or the pane the sidebar
  was on.
- **One `Field` names every playlist.** `RootView::name` is shared by the pane's new-and-rename row and
  the picker's *or a new one*, so only one of the two can be open: `name_a_playlist` closes the picker
  and `hold_for_a_playlist` closes the naming row. The field emits `Submitted` on enter and
  `RootView::name_given` decides from whichever is open whether the name creates, creates-and-adds,
  renames, or saves and revises a search. Which heading draws the row is the `Naming` variant's:
  `Naming::Query` under the *tracks* heading, where the gesture is and the search box above is still
  live, and `New` and `Rename` under the playlists heading — one entity cannot be in the frame twice, so
  each heading filters `naming` for the variants the other leaves. `RootView::contact` is the third
  `Field`, the Online card's, filled by `Field::hold`: `set_content` with the preedit cleared (`clear`'s
  shape), so the saved contact is in the box when the window opens and is put back trimmed on enter.
  `contact_given` stores `Setting::Contact`, moves the global, reports it is sent from the next
  request on — the store reintroducing every client (`online.md`) — and hands focus back; escape and a press outside leave it through `leave_contact`, ahead of the naming
  row in `dismiss_search`, and `editing` counts it so the typed keys stay off while the caret is in it.
  The order and the cap ride beside the name as chips, a saved query being a search, an order and a row
  cap, the window otherwise only ever saving the first. *Edit search* on a saved query is
  `RootView::revise_search`, putting the query's text back in the search box, making the pane Tracks and
  filling the chips from what was saved, so revising one is the gesture that saved it.
- **A run of rows is dragged where it goes, and the keyboard reaches one before moving it.**
  `views/reorder.rs` owns how rows move: `Shift` names which list is edited (`Queue`, `Playlist` or a
  browse `Listing`), `Step` is up or down and `Step::landing` is the one arithmetic behind the arrows,
  the drop and the keys — what `resonate-ui`'s own tests cover — stepping off `Span::first` going up and
  `Span::last` going down, so a span and a row move by one rule. A row is both drag source and drop
  target, so dropping a span on row `to` is the arrows' `Command::Move` — take out and put back, never a
  swap. The payload is `Carried`, and the `Ghost` under the pointer is the first row's title and how
  many more ride with it, padded by the offset gpui hands the constructor so it rides at the pointer,
  not where the row began. `RootView::reach` is a `Reach`: a `Shift`, the anchor the reach opened on and
  the row it moved to, so a reach left in the queue is not drawn in a playlist, living only for the run.
  `up` and `down` move it and collapse it — the queue opening on the playing row, a playlist on its
  first — `shift-up` and `shift-down` grow it from the anchor, `home` and `end` take it to either end,
  `pageup` and `pagedown` move it by `rows_a_page` — the list's viewport height over
  `theme::row_height()` (for a grid, over `theme::grid_row()` times `grid_columns()`), read off
  `UniformListScrollHandle`'s `last_item_size`, whose `item` is the viewport and `contents` the whole
  list — `ctrl-a` reaches every row, `shift`-click every row between, `alt-up` and `alt-down` move what
  is reached with the reach following, `enter` plays its first row and `delete` takes the reached rows
  away through the ✕'s `drop_rows`, gated on `Rows::are_edited` in a playlist as the ✕ is. `backspace`
  over a reached row drops it too rather than silently editing the search, as `typed` once did:
  `drop_reached` answers whether it acted and the query is edited only where it did not — unless a
  type-ahead jump put the reach there, which backspace leaves standing.
  `RootView::acting_on` is what every row gesture asks: the reach where the pressed row is inside it,
  that row alone where not, so the ✕, the arrows and the queue marks need no selection of their own. It
  is drawn as a 2-unit accent edge and a faint accent wash every reorderable row reserves in
  `theme::UNMARKED`, so the mark costs no layout when it moves. A saved query draws none of it, its
  order being the query's. A drop is the one gesture asking `acting_on` nothing: `reorder::movable`
  takes the row it stands for beside the `Carried` it would hand over, a span landing on the row the
  pointer let go over, not on what that row would have carried had it been dragged.
- **A drag scrolls the list, and a held pointer keeps it scrolling.** `reorder::follows_a_drag` puts an
  `on_drag_move` on the div wrapping each `uniform_list` and hands `RootView::creep` a `Creeping` —
  which way, whose scroll handle and how many rows — while the pointer is within `DRAGGING_EDGE` of an
  edge, and `None` where not. gpui reports a drag only when it moves, so the move alone would stop once
  the pointer settled: `creep` scrolls one row past `ScrollHandle::top_item` or `bottom_item` then runs
  `creeping_on`, a task repeating every `DRAGGING_STEP` (60 ms) until `creeping` is cleared or
  `App::has_active_drag` says the drag is over. One task serves both ends and both panes, reading
  `creeping` each turn rather than capturing it, and it is the one clearing the cell, so a live cell and
  a live task mean each other.
- **The settings pane is a rail of categories over a column of sections, and one table says what a
  section is.** `views/settings/` is a folder, the pane holding two pieces of arithmetic worth testing
  without a window. `find.rs` is the vocabulary: a `Category` — Output, Processing, Equaliser, Library,
  Online, Desktop, Appearance, About — and a `Group`, one per setting, carrying its category, title,
  hint, the words it is *also* called and the `SettingKey`s putting it back. Nothing else enumerates the
  sections: `Category::groups` filters the one table, `mod.rs::group` dispatches a `Group` to the method
  drawing it, and tests hold every group to standing in one category, a title nothing else claims, a
  hint of its own and no key claimed twice. The rail is `theme::settings_rail()` of icon rows down the
  pane's left edge, `RootView` remembers which is open for the run, and the body is a column of
  `kit::section`s — a header strip carrying the title, the revert mark and the info mark over a body —
  rather than the flat cards the pane drew. The body scrolls under `RootView::settings_scroll`, a
  `ScrollHandle` of its own, and `show_settings` puts it back to the top whenever the category changes:
  gpui keys a scroll offset by element id, and one `"settings"` id across every category left the next
  page opened as far down as the last was read.
- **Files dragged from outside are weighed while over the window and copied when dropped.**
  `views/dropping.rs` is the whole of it. gpui turns a file drag into an `ExternalPaths` drag, but
  only the element dropped on hears of it ending — nothing tells a view the drag *left* — so the
  root's `on_drag_move::<ExternalPaths>` sets `RootView::incoming` (the paths and
  `resonate_library::weigh`'s read of each) and starts `watching_the_drag`, a 100 ms timer clearing it
  once `App::has_active_drag` is false. While `incoming` is set `drop_overlay` draws over the window
  — a scrim, a dashed card listing up to six names with *copied*, *beside a song, copied with it*,
  *not audio, left out* or *not there* beside each, and the destination, *copied and filed by your
  layout* where `file-dropped` is on — and is the `on_drop` target, occluding what is under it.
  `verdict` is the one decision the overlay and the drop share: `Ready`, `NoFolder`, `FolderGone`,
  `NothingToTake` or `Busy`, the last four drawn in the failure colour and, dropped, a toast; a drop
  with no `music-folder` also opens Settings on Library. A ready drop starts `take_in` into
  `ResonateApp::music_folder`, `copying_pill` draws its progress — files and bytes, with a *Stop* —
  until it lands, `told_of` toasts what it did, and where anything was copied the root reaching the
  folder (or the folder, added as one) is scanned through `add_roots` — with `file-dropped` on, the
  landed songs handed to `file_once_scanned` first, filed once that scan lands (`library.md`'s *Taking
  files in*). gpui gives a test no way to
  build an `ExternalPaths`, so `driven.rs` calls `dragged_over` and `dropped` on the `RootView`
  rather than simulating the platform's drag.
- **Discord is two Desktop groups, and they write a global before a file.** *Discord* is the switch and
  the application id; *What Discord shows* is how much text, which picture, the icon, the bar and
  whether a pause keeps it. Each control changes `ResonateApp::presence`, stores its `Setting` and hands
  the whole `Presence` to `Present::follow`, so the publisher starts, stops or re-sends at once. The id
  and the icon are `Field`s read on enter through `AppId::parse` and `Icon::parse`, and a text neither
  reads is a `Notice::Trouble` storing nothing rather than a key the next start refuses; a blank one
  clears the key.
- **The body is as wide as a number, never `w_full` under a `max_w`.** taffy fixes a flex item's height
  from a measure taken before a percentage width resolved, so a column clamped so is measured with its
  text *unwrapped*, its `content_size` comes out short and a long category's bottom cannot be scrolled
  to — the lyric row's trap, with the same answer: a pixel width wherever a column's height depends on
  how its text wraps.
- **A boolean is a `kit::switch`, a choice is `kit::segmented` and only a device or a palette is still
  a row.** The four near-identical two-variant enums the pane carried are gone: a switch names what it
  does and what off means and takes the whole row as its hit area, and a `Choice` is a trough of
  segments. `kit::section`, `kit::field`, `kit::segmented`, `kit::switch`, `kit::preview`,
  `kit::dot_swatch` and `kit::choice_row` with its `kit::radio` are the vocabulary, and the pane draws
  none of them itself — `kit::button`'s rule. **`kit::segmented` answers with the trough itself and the
  caller hugs it.** It answered with a wrapper round the trough, so every segment landed beside the
  trough rather than in it — bare labels with an empty six-pixel pill to their left, in every selector.
  A builder returning a parent its children will not reach is the shape to watch for.
- **A device reads as what it is and what it takes, and its node name is behind the pointer.** Each
  choice, *Follow the system default* included, is a `kit::choice_row`: a `kit::radio` held
  `kit::level_with_the_choice` beside a column whose first line is a `kit::choice_name`, so the radio
  sits on the name rather than midway down three lines and the default and every device line up on one
  edge. The chosen row takes the whole accent as its border over an accent wash, as the worn palette
  card does, and that and the filled radio are the only two marks — the speaker every row led with
  (identical on every row, saying nothing else) and the chosen one's 2-unit accent edge were a third and
  fourth. The name is `flex_1` and ends in an ellipsis — sized by content, a clamped name collapsed to
  nothing beside its badges, and `truncate` alone sliced its last letter — and the badges ride the same
  line after it — `DEFAULT`, `UNPLUGGED`, `IN USE` and the verdict on the playing track, `TAKES … AS IS`
  or `CONVERTS` — so no description pushes them off the end, and the verdicts down the list read as one
  column. Under the name are two `kit::details` lines, parts divided by a faint `·`: how the sink is
  reached, in words — the port, the profile the card is switched to and whose volume it is — as faint
  `kit::detail`s, each ending in an ellipsis where it outruns its line (the default entry's *Whatever
  PipeWire routes to, which is …* was sliced by the card's edge), and what it takes, in figures — depths and rates through `format::depth` and
  `format::kilohertz`, comma-listed — as `kit::figure`s. What it replaced drew all five as identical grey
  mono badges in one wrapping cloud, words in the figures' face, listed with a slash that read as
  shorthand. A port with nothing plugged in is a badge and a `muted` name rather than a parenthesis, and
  the default entry says which device PipeWire routes to now where the graph names one. The node name is
  still what the settings file holds, so it is what the row `names` itself; `SinkInfo::is_hardware` is
  drawn nowhere, reading `device.api` off the node, which an ordinary ALSA sink lacks.
- **A section holds a subject rather than a control.** The theme shelf and accent swatches are one
  *Colour* section with a `kit::field` label over each half, six one-control sections reading as a list
  of switches rather than a page. A group's hint is written once, in the table, and the header's info
  mark is the only place it is drawn — `views/hint.rs` is the view behind it, a gpui tooltip being built
  from an `AnyView`, not a string.
- **Every group ends in a line saying what it does, and a choice says what the chosen option does.**
  `Choice::meaning` is required of every choice rather than defaulted, and `RootView::choices` draws the
  chosen option's meaning under its trough as a `note` — the resampler's filter, a dither, a ReplayGain
  mode, a buffer depth, a Discord picture — while `detail` stays the hover for figures such as the
  filter's taps. A group that is a list or a row of buttons — the devices, the music folders, the scan,
  the bindings, the three About readbacks — ends in a `note` of its own, every note the one `note`
  builder at `text_sm`, the accent line included. *Lossy sources* is *Artificially enhance lossy files*,
  carrying the `EXPERIMENTAL` badge `Group::is_experimental` puts in a header, what it adds being a guess
  at what the encoder threw away; each of Off, Repair and Repair and extend says in words what it takes
  or adds, and `LOSSY_ONLY` under them says which files are touched and that a repaired one is no longer
  bit-perfect.
- **A palette is shown rather than named.** `kit::preview` is a strip of four bands — the theme's
  sidebar, its panes, what it raises above them and the accent that would be worn — under the palette's
  name inside a card taking the accent as its border when worn. An accent is a round `kit::dot_swatch`
  carrying a check in whichever ink `theme::ink_over` says reads on it. Appearance is the one category
  with nothing behind it but the window: a choice is worn through `theme::wear`, saved through the
  `Settings` seam and drawn again by `cx.refresh_windows()`, so no `Command` is sent and the pane marks
  what is worn rather than what it last sent. Text size goes the same way and moves every measure.
  *Window buttons* is the third group — the chrome is how the window is drawn, where Desktop is what the
  player tells the session — worn through `RootView::show_window_buttons` onto the global rather than
  `theme`, a button being no colour or measure. *The volume wheel* is the fourth, one switch worn onto
  `ResonateApp::scroll_volume` and written as `scroll-volume`; *Scrollbars* the fifth, Always, While
  scrolling and Never worn onto `ResonateApp::scrollbars` and written as `scrollbars`; *Sidebar tabs* the
  sixth, three switches worn onto `ResonateApp::tabs` through `RootView::show_tabs` and written as
  `suggestions-tab`, `missing-tab` and `tab-counts`, the last taking the figure off every sidebar tab.
  `Pane::is_shown` is what a tab being off means: the sidebar and `stepped_pane` pass the pane over and
  `set_pane` lands on the tracks instead, so neither a way back nor a button reaches a hidden pane, and
  hiding the pane in front moves off it at once.
- **gpui scrolls a region and draws no bar for it, so `views/scrollbar.rs` does.** `Scrollbars::of` reads
  the setting once where a pane is built, and `vertical`, `horizontal` and `around` answer a bar over the
  region's edge — or an empty absolute div under `Hidden`, so a pane is built one way either way. A bar
  reads the region's own `ScrollHandle` — a `uniform_list`'s base handle — and paints the thumb in a
  `canvas`, the offset moving between renders and only paint seeing where it is now. **Nothing a bar
  knows survives a render**: a `Cell` made in the builder is new each frame, so a hover `on_hover` wrote
  into one was gone by the redraw it asked for. The bar lights by weighing `Window::mouse_position`
  against its own bounds in paint, and the paint writes those bounds into a cell the same frame's
  listeners read, so a track press and a drag's grip measure the bar the pointer is over. Where on the
  thumb it was held is taken when the drag *starts*, in `on_drag`'s constructor, not on the press, a
  track press moving the thumb and redrawing before the drag begins. **The lyrics pane's bar is drawn only
  while the sheet is moved.** A following sheet glides its whole length, and a thumb standing beside the
  words read as a second, busier line to watch; `Scrollbars::vertical_while` paints the thumb only where
  the pane says it moved — `LyricsModel::moved_by_hand_lately`, `BAR_LINGERS` after a wheel turned it —
  or the pointer is on the bar or its thumb is held. The glide is not a movement for this, and
  `is_turning` keeps frames coming until the linger is out, so the thumb goes on its own.
  **`ScrollbarMode::AutoHidden` does the same for every bar, from the offset alone.** A bar that would
  always be drawn is drawn `Shown::WhileScrolled`: its paint keeps the last offset it saw in element state
  under `LAST_SCROLLED` — the one thing a bar carries between frames, a `Cell` not surviving, read on
  every paint whether or not the pointer already lights the bar, gpui dropping a state no frame reads —
  and paints the thumb while that offset moved within `SCROLLED_LINGERS`, or the pointer is on the bar or
  its thumb held. The paint seeing the offset move spawns one timer refreshing the window once the linger
  is out, so the thumb goes with no frames asked for between. The lyrics bar keeps its own rule under
  every mode but `Hidden`, the glide moving its offset without being a scroll.
- **A setting moved off its default says so, and the mark is what puts it back.** `defaults.rs` is the
  arithmetic: `Standing` is everything the pane can put back — the `OutputSettings`, the `Appearance`,
  the online, lyrics, listening, resumption, desktop, Discord, convolution and window switches and
  choices, and
  whether each typed value (contact, keys, tokens, template, inbox, music folder, Subsonic, TIDAL) is given — `differs` weighs
  it against `Standing::as_built` (`EngineConfig::default()`, `Appearance::DEFAULT` and the rest), and
  `puts_back` answers the commands restoring it. A section whose value differs grows an `Icon::Undo` in
  its header; the footer carries *Reset <category>* for every category with a key to put back, and
  About's *Reset everything* arms on the first press and fires on the second, a modal not being in this
  crate's vocabulary. Putting a setting back is two moves: the default is sent and the key *taken out of
  the file* through `SettingChange::Forget`, so a build whose default later changes is followed
  rather than pinned. **A setting is written off the render thread, and a run of them is one write.**
  `RootView::store` and `RootView::forget` hand a `SettingChange` to `SettingsWriter`, which sends
  what gathered to a thread of its own (`resonate-settings`) as one batch, one batch in flight at a
  time and whatever arrives meanwhile the next; `Settings::apply` takes the whole batch. So *Reset
  everything* — some fifty changes — is a write or two, where each was a locked read-modify-write
  with two `sync_all`s on the UI thread, and dragging a slider costs the frame nothing. A batch that
  failed is one toast; dropping the writer, as the window closes, sends what is still gathered and
  joins the thread, so nothing changed at the last moment is lost. A group with no key — the folders, the scan, *Look up now*, all three of About's — offers no
  mark, which a test holds.
- **A setting is found by typing rather than by remembering which tab holds it.** `RootView::finding`
  is the pane's own `Field`, reached by `ctrl-,` or a press, counted by `RootView::editing` so the
  transport keys stand down while the caret is in it, cleared-and-blurred by escape ahead of the contact
  field in `dismiss_search`. `Narrowing` matches every word against a group's title, hint, category and
  other names — `sink` finds the device, `latency` the buffer, `sacd` DoP — so a setting is reachable by
  what another player calls it. While a query is in force the heading reads *Found*, the rail draws each
  category's match count instead of a selection, and the body draws the matching sections from *every*
  category under a category eyebrow, a filter inside the front tab alone being a worse tab. **What is
  found is ranked by how it answered.** A group answers each word by its title, another name, its
  category or its hint, in that order of strength, weighed as its weakest word; a category leads by its
  best group and a group within it by its own weight, so *dither* opens on Dither rather than a shaping
  hint mentioning it, and no category is drawn under two eyebrows —
  `a_setting_named_by_the_words_leads_one_whose_hint_merely_mentions_them`.
- **A right press answers with a menu, and `views/menu.rs` is the whole of how.** `Menu` is a position
  and a run of entries, each an icon, a label, an optional key and a closure;
  `Menu::at(…).does(…).under(…).apart()` builds one and `menu::opens_a_menu` attaches it to any element,
  so a pane says what a press offers and never how a menu is drawn. It renders as
  `deferred(anchored().position(at).snap_to_window_with_margin(…))` over an occluding scrim taking the
  outside press, and is one `Option<Menu>` on `RootView` with one arm in escape's chain, after the
  sheets and before a toast, and one at the head of `dismiss_search` — `adding`'s and `magnified`'s
  shape. It is not built on `tooltip`, which `hint.rs`'s source-walking
  test forbids and which neither a press nor a scroll takes down. It needs no key context: `up`, `down`
  and `enter` already name actions this window handles, so `reach_row` and `play_reached_rows` step and
  press the menu before reaching for a row. **`on_click` never fires for a right press**, gpui 0.2.2
  recording a pending press only for `MouseButton::Left`, so the press opening a menu plays, scopes or
  seeks nothing and no listener sharing an element with a menu needs a guard. A guard reading
  `is_right_click` once stood on every such listener and could never answer false; it was taken out. A
  gpui routing other buttons to `on_aux_click` keeps the same promise. A track row plays, queues either
  way, holds for a playlist, reaches the artist and album, shows its file in the file manager and copies
  the path, title, artist and album — *Go to album* having nowhere else to live, a track row having no
  album column. **Where a row goes is read when the entry is pressed, not when the menu opened**:
  `Menu::reaches` takes the row's catalog id beside the album and artist it drew, and the press asks
  `LibraryModel::standing_of` for the track as the catalog holds it now, so a menu left open across a
  scan that gathered the album away opens the album it was gathered into. A row no scan has seen, or one
  the catalog has since dropped, goes where the menu said. `Menu::offers_the_file` is that last group,
  written once for the tracks, queue and playlist rows and handed the row's `Called`, whose album
  `RootView::album_named` reads off the catalog's album or, for an unscanned row, the tags the player
  read; an empty name is not offered. *Show in the file manager* is gpui's `reveal_path`, asking the
  portal's `OpenURI.OpenDirectory` for the folder with the file selected and falling back to opening the
  folder. The playback bar's and inspector's names are a left click to their album or artist, so a right
  click copies that name whole — not the cut the bar draws — through `copied_on_a_right_click`, a toast
  saying what is on the clipboard. An album cell, an artist row, the two covers, a queue row, a playlist
  row, a column header, the search box and a lyric line each carry their own. A sidebar row, a settings
  control and a missing row deliberately carry none: each would offer only what one press does.
- **A copy is handed to the compositor over `ext-data-control`, not through gpui.** gpui 0.2.2's Wayland
  `write_to_clipboard` sets the selection under the serial of the last *key* press, so a copy made from a
  menu with the pointer — the path, a lyric line, a share — carried no serial or a stale one, and KWin
  kept what the clipboard held before. `clipboard::copy` is the one way the window copies: it opens a
  connection of its own, binds `ext_data_control_manager_v1` and the seat, sets a source offering the
  text types and waits `ANSWERED_WITHIN` for the round trip saying the compositor took it, then a thread
  named `resonate-clipboard` answers each `send` with the bytes until the source is cancelled — which the
  next copy, ours or anyone's, does, so at most one such thread lives. Where the compositor offers no data
  control — GNOME keeps it to privileged clients — it falls back to gpui's own write, which works
  whenever the window was last reached by keyboard. **The UI thread never waits for the answer, and a
  late one cannot overwrite the fallback.** `copy_through` starts the thread and a foreground task
  racing its oneshot answer against an `ANSWERED_WITHIN` timer; the fallback is written only once the
  compositor refused or the timer won. Which side got there first is one `Claim`, an atomic settled
  once: the thread claims it just before `set_selection`, the task gives it up when the timer fires,
  and whichever loses stands down — a thread finding the claim given up destroys its source unset, and
  a task finding it claimed waits for the round trip rather than writing. It used to block the frame
  for up to 250 ms on `recv_timeout`, and a thread answering after that set the selection over the
  text the fallback had just written. **A later copy wins, however the round trips land.** Each copy
  takes a ticket from `Copies`, a gpui global counting what the window has asked to copy, and only the
  newest may claim the compositor or write the fallback; a thread's claim, `set_selection` and round
  trip run under `Issued::handing_over`, one lock across every copy, so an older copy whose connection
  was slow finds itself outrun and stands down rather than setting the selection over a quicker
  second one (`a_copy_outrun_by_a_later_one_neither_claims_the_compositor_nor_writes_the_fallback`).
  The counter is per `App` rather than a static, so the tests' windows do not outrun each other.
  `clipboard.rs`'s tests drive both sides with a stand-in offer. It is `wayland-client` and `wayland-protocols` with
  `staging`, both already linked by gpui, and no `unsafe`: the descriptor arrives as an `OwnedFd` and is
  written through a `File`. It was proved against a headless `kwin_wayland --virtual` on a socket of its
  own, read back with `wl-paste`, and is not a test, a copy in a desktop session being the listener's
  clipboard.
- **A scope remembers where it was opened from, and its way back says where that is.** `Wayback` is the
  pane, the selection, the row the list was left on and the place's name, in a bounded `WAYS_BACK` stack,
  and every scoping gesture goes through `RootView::opened`. The name is read by `RootView::here` as the
  scope is left — the album's title, the artist's name or the pane's label — because by the time the way
  back is drawn the library has moved to the new scope. `goes_forward` keeps the bounded forward stack;
  going back saves the current page there, going forward saves it back to `came_from`, and opening a new
  scope or `show_everything` clears the forward stack. When Appearance's *Mouse navigation* is on
  (`mouse-navigation`, default true), the root handles the mouse's back and forward buttons through
  these same stacks. The way back is `kit::way_back` at the page's top
  left, a chevron and that name — *‹ Tracks*, *‹ Hypnotize* — and where nothing was left behind it reads
  the category the page stands under and goes there, clearing the scope. It replaced a ghost *Show all*
  among the right-hand actions, which cleared the scope rather than returning and was the one control not
  looking like the way out. Escape is `RootView::step_back` too. **Where a list was left is a row and how
  far into it.** `LeftAt` reads the scroll offset against the pane's row height — `row_height_of`: the
  grid's row for the albums and an artist grid, the tall row for the artists list, the plain row for the
  tracks — as the row it stands in and the fraction scrolled past, and the landing sets the offset back
  from the same pair at the rows' height then, so a list is back on its pixel and one whose rows grew with
  the text size lands on the same row as far into it —
  `the_way_back_lands_on_the_pixel_the_list_was_left_at`, driven, and
  `a_list_whose_rows_grew_lands_on_the_row_it_was_left_at_and_as_far_into_it`. The landing waits on
  purpose: the listing is read on the background executor, so `land_where_it_was_left` holds it until
  the pane has that many rows. All three lists land — the albums grid, the tracks and the artists — each
  through a scroll handle of its own, `album_rows` the grid's, so the row is read off the list that was
  showing rather than `track_rows`. `landing_on` is set only for a pane that
  `Pane::lands_where_it_was_left`, and `set_pane` clears it, so a landing no pane reads cannot wait for
  the next time the artists pane opens and jump it to a stale row.
- **What the window names a queued row by is weighed against the row, not only its id.** The engine
  mints ids from `u64::MAX` down afresh for every restore, unscanned playlist row and doubled row, so two
  files can carry one id a load apart; `LibraryModel::track_of` keeps the location and span beside the
  name it read and reads again where they disagree. `album_title` answers for any album — the scoped one,
  the listing's, or one read by id into a bounded `read_albums` the next listing load throws away — so a
  search or page leaving the playing album out does not blank the playback bar, the magnifier's caption
  or the queue's album order. **What the queue pane weighs every row for is read off the render
  thread.** `queue_heading`'s total is measured once per queue and library revision, in the background
  through `queued_rows` (`QueueMeasure`), the heading keeping the last total until the new one lands;
  and a sort chip keys every row in the background too — `ordered_rows` reads the tracks in one pass
  and their album titles in another (`Library::album_titles`) — sending the order only if the queue is
  still the revision it keyed. Both read on the UI thread once, one row at a time past `NAMES_HELD`'s
  4 096, so a counted play under a 20 000-row queue froze the window for some 40 000 reads.
- **A row is keyed by what it holds, not where it stands.** gpui keeps a tooltip, a hover and a drag in
  element state under the element's id path, so a row keyed by index handed row N's tooltip — a
  favourite's *Take out of favourites* among them — to whatever row an edit moved into place N. A queue
  row is keyed by its queue id, a track row by its `TrackId`, an artist row by its `ArtistId`, a missing
  row's want mark by its release track or the found recording, and a playlist entry — which may hold one
  cut twice — by `listing::keyed_by` over the cut and its place, so a row now holding something else is a
  new element with nothing carried over.
- **A row gesture reads the row the pane drew, not the row the list holds at that index.** A playlist
  narrowed by a search is not reached, so its row menu's *Take out of the playlist* drops
  `Span::one(entry.position)`, the row's place in the list, as its ✕ does — `index` is its place in the
  narrowed view. Enter on a reached row inside an album goes through `LibraryModel::played_from`, turning
  a display row into the track it draws and answering nothing for a disc heading or a missing row, as a
  click reads `ListedRow::Held` through `played_from_held` — the tracks pane's rows are drawn
  `Plays::AsTheListingIsDrawn`, the favourites' and a suggestion's `Plays::TheseRows`.
- **Every control in this pane is reachable from the keyboard, through gpui's own ring.**
  `views/focus.rs` holds one `FocusHandle` per control id and forgets those a frame stopped drawing, so a
  redrawn control keeps the caret; the ring is `Window::focus_next` and `focus_prev` over the tab stops
  gpui inserts in paint order, so nothing re-implements an order. A control carries `.track_focus`,
  `.tab_stop(true)` and `key_context(CONTROL_CONTEXT)`, and that context is the whole key plumbing:
  `space` and `enter` are bound under it and fire the control's *own* `on_action`, escape lets go, and
  every window binding that carried `!Search` carries `!Search && !Control` — so the transport stands
  down while a control is reached as it does for the search field. `tab` and `shift-tab` are bound with
  no predicate, a key moving focus needing to move it from inside a text field too. Focus is drawn as
  hover is: the accent on the border where a control has one, an accent wash where not.
- **What is in this build, and where it keeps things, is a category rather than a window.**
  `WindowKind::Preferences` and `WindowKind::About` are gone — nothing constructed them, which
  `errors.md` calls a defect. About reports the version, the two faces `fonts.rs` settled on, how many
  sinks the graph advertises and whether a reference started, and names the settings file and the
  catalog, reaching it as `Places` on `run` beside the `Settings` seam in `Stored`. They are readbacks,
  not settings: `library` has no control because it names the database the pane reads from, and the
  settings file's path is what `--config` chose.
- **DoP is a claim about hardware, so the pane says so in as many words.** `Command::SetDop`,
  `OutputSettings::dop` and `Setting::Dop` exist because the key did: `EngineConfig::dop` and
  `ConfigKey::Dop` were readable from `config.toml` and reachable from nowhere else — no command, no CLI
  flag, no control. The switch is off by
  default and its hint says what `audio.md` says: nothing in ALSA or SPA advertises DoP, only the DAC's
  own detector knows, and a DAC that does not decode it plays the markers as full-scale white noise.
  `pipeline.rs`'s `no_setting_the_pane_offers_can_produce_a_marked_plan_with_a_stage_in_it` already held
  `dop: true` fixed, so its claim covers the pane now it can set it.
- **A value off a choice's table is said, never left unlit.** `RootView::choices` takes the value in
  force, not an `Option`: a segment is lit where it equals one of `Choice::ALL`, and where none does
  `said_under` writes *… is in force, which is none of these.* in place of a meaning, through
  `Choice::in_force` — the label by default, which a closed enum always has, and the figure itself for
  the choices whose key reads any number: `buffer-ms`, `listen-for`, `bluetooth-lead-ms`,
  `bluetooth-awake-s`, the two ReplayGain trims and a `history-kept` of days the table does not offer.
  Those choices are newtypes over what the key holds (`BufferDepth(Duration)`, `ClipLength(Duration)`,
  `PreAmp(Trim)`), not enums of the rungs, so the pane cannot turn an off-table value into `None` and
  forget to say so; each module's tests hold the sentence. The resampler group prints `SincParams`'
  four numbers on hover rather than describing a level in adjectives. A device named in the file but
  absent from the graph marks no row, and the group says so. The volume is debounced so a held key
  rewrites `config.toml` once rather than per repeat, and it is the playback bar's control, not a
  setting here.
- **Whether a control can be pressed is still the builder's decision.** `kit::Press` and the
  `settings::action` helper are unchanged in kind: a greyed control is built greyed, takes no listener
  and joins no focus ring, because gpui's `hover` carries a `debug_assert!` a second call would trip and
  a control nothing can press should not be a tab stop.
- **A pass that rewrites the files is armed before it runs, and the preview is what arms it.**
  *Organising* and *Tagging* are the two, one shape: a *Preview* running the pass with `Pass::Preview`,
  an *Apply* greyed until a preview has landed and then taking two presses — the second under a note
  saying what the first armed — and a *Stop* drawn only while the pass runs. Each also offers *Put the
  last run back* wherever the library says a run is noted — `walks_back` and `tags_walk_back` — armed the
  same way under `walking_the_filing_back` and `walking_the_tags_back`. `RootView::moving_the_files` and
  `writing_the_tags` are the other two arming flags and `disarm` clears all four, so leaving the pane or
  changing category puts an armed control back. So does the pointer leaving the settings body: its
  `on_hover` lowers every armed press — *Reset everything* and the vault's *Keep* with these — the moment
  it reads false, a transition, so a press armed from the keyboard with the pointer already elsewhere
  stays armed until the pointer has been in and out. A finished reset's note is no arming and stays. Both
  run through `Work`, what `LibraryModel::is_busy` reads, so a scan, a forget, an organise and a tag write
  cannot overlap and each greys the others' controls; `Library::retag` takes the same `Walk` guard
  `Library::scan` and `Library::organise` do, so a second caller would be refused anyway and the greying
  stops it being asked.
- **A preview is taken down once the catalog it read has moved.** `Planned` is what each of *Organising*,
  *Tagging* and the vault's *Import* holds: `Not`, `Shown` with the `resonate_library::CatalogStamp` taken
  as the preview started, or `Outdated`. Every reload's `landed` weighs a shown preview against
  `Library::plans_stamp` and, where the stamp moved, drops its rows and counts and greys *Apply* again,
  the group saying the catalog moved rather than asking for a first preview. The stamp is a counter of its
  own on the writer connection — temp triggers over the columns a plan reads, `PLANNED_FROM` in `db.rs` —
  beside `PRAGMA data_version` for another process, so a lookup landing a release or a scan reading a file
  anew takes the preview down while a counted play or a favourite does not. An arming is read as the flag
  *and* a shown preview, so a preview taken down leaves no half-armed *Apply*.
- **The Tagging group draws the plan rather than a count of it.** `listed` is the first `WRITES_SHOWN`
  of `Retagging::writes`, one raised row per file carrying the file name over the fields that would be
  written — `cover art` among them where a picture would go — with *and N more* where the pass named more,
  then the summary note. A count alone is the wrong preview for a pass whose unit is a field: a listener
  arming *Apply* wants which files are touched and what goes in them. The count is still the pass's own —
  `would write` and the fields and pictures summed off the plan before an apply, `RetagStats` after — so
  the note cannot disagree with the rows.
- **What a scan skips can be read again on purpose, one part at a time.** The Library category's
  *Refreshing* group holds *Read every file again*, the ordinary scan with `incremental` off —
  `Reading::Everything` beside `Prompted` on `LibraryModel::start_scan` — so every file is probed again
  whatever its size and mtime say, and the names, lengths, sheet cuts, embedded covers and search index
  follow while each row keeps its id, plays and favourite; and *Look for missing covers*,
  `Library::ask_again_for_covers` and a lookup, whose sweep asks the archive for every album with a
  release and no picture. It is greyed with the scan's `is_busy`, the covers button with `can_enrich` as
  well. What MusicBrainz answered is Online's *Refresh all*, which the group's note points at.
- **The library's roots are edited through the desktop's file picker, or typed.**
  `App::prompt_for_paths` with `directories: true` is the XDG portal, and a machine without one says so
  and points at the field beneath: `RootView::typed_root` is a `Field` like the organising template's,
  *Or type a folder, then press enter*, and `root_typed` reads a leading `~` as `$HOME`, refuses what is
  not a directory in the pane's own notice and hands a directory to the picker's `add_roots` — so a
  session with no portal adds a folder from the window, not only through `resonate scan`. Adding scans
  just the folders named, which registers them; *Rescan* walks the registered roots, and dropping one
  goes through `Library::remove_root` and forgets every track from it. The roots ride in the one `Loaded`
  snapshot with the albums, artists and tracks, so nothing else needs to know when they change. They live
  in the library database, not `config.toml` — the one setting not going through the `Settings` seam.
- **The inspector renders the engine's `StreamDigest`, never a second read of the file.** `audio.md` has
  how the digest is sampled; `resonate-ui` takes no dependency on `resonate-codec`. It opens with the
  signal path as three `stage` cards — *Source*, the codec in the lossless colour over the depth, rate,
  channels and container; *Processing*, what the output mode did and the gain applied; *Output*, the mode
  in its colour over what was negotiated and the sink — then the detail cards, the bitrate graph and the
  tags. It follows the *playing* file alone, inspecting a selected track meaning `probe_stream` from the
  window. The live profile restarts on every rebind — a seek, a sink switch, a renegotiation — a
  `ProfileBuilder`'s windows being sequential and unresumable mid-span, so the graph covers the decoded
  span since the last rebind, what `StreamDigest::profiled_from` names. The digest is rebuilt once per
  closed window, once per second of decoded audio, and the decoder runs ahead of the sink by the ring's
  depth, so the graph leads what is heard. It is a line, not bars: `plot::stroke` draws a polyline over a
  `plot::wash` from the accent down to nothing, a listener reading the rise and fall between windows
  rather than any one height, and a `div` per column cost a layout node per point. The line is condensed
  at the digest's rate, not the frame rate: `PlayerModel::condensed` holds the reduced series and throws
  it away when the poll swaps the digest for a different `Arc`, because `StreamProfile::series` runs to
  `MAX_WINDOWS` — a whole day — and reducing it to `GRAPH_COLUMNS` points sixty times a second is a walk
  and an allocation a frame for a picture changing once a second. A one-window profile is drawn as a level
  line, one reading held across the span.
- **The visualiser draws what the tap says is being heard, and its arithmetic has no gpui in it.**
  `Pane::Visualiser` sits under `Section::Playing` beside the lyrics and the inspector — the same six
  edits the panes before it were — and follows the playing track alone: with nothing playing it is
  `kit::empty` under `Icon::Visualiser`, a silhouette of bars under held peaks, and a DoP stream is
  `kit::empty` saying it carries markers rather than samples. The heading is the lyrics pane's — title
  and artist through `opens` — with a `kit::figure` reading back what is analysed, *4096-point · 48 kHz*,
  the hint, and a `kit::segmented` of *Spectrum* or *Scope*, living on the entity for the run.
  `spectrum.rs` is the arithmetic, tested as `curve.rs` is: a radix-2 real transform by hand — a
  half-length complex one over the bit-reversed even and odd samples, split back into the real spectrum —
  under a periodic Hann window, scaled so a full-scale sine on a bin reads 0 dBFS; sixth-octave bands
  across the equaliser's `RESPONSE_FROM_HZ` to `RESPONSE_TO_HZ`, placed by `across_at` and labelled by
  `marked_frequencies`, so both plots read 100, 1k and 10k at the same places; a band narrower than a bin
  read at its centre between the bins either side, one past Nyquist left on the floor. Each band is its
  loudest bin tilted up `TILT_DB_PER_OCTAVE` (3) about 1 kHz, so pink noise stands level and a mastered
  record does not slope into the treble, drawn between `FLOOR_DB` (−78) and 0 over faint lines every
  12 dB — the levels carrying no figures, a tilted reading being dBFS only at the pivot. A bar rises at
  once, falls at `FALLS_DB_PER_SECOND` (40), and its peak is held `PEAK_HELD_FOR` (700 ms) before it
  follows, all in wall-clock time so a late frame falls further rather than slower. The window is sized
  by the rate, about 85 ms: 4 096 points at 44.1 and 48 kHz, up to 32 768 at 384. The scope is 20 ms of
  left over right, starting on the first rising zero crossing of their mid in the first span of a window
  twice that long, so a steady tone stands still.
- **The pane follows the display's clock while it has sound to draw.** A zero-size canvas asks
  `Window::request_animation_frame` whenever the transport plays and the engine hands the pane a tap, as
  the lyrics pane follows a synced set, so bars and scope move once a display frame — 120 a second on a
  120 Hz panel — rather than on the 16 ms poll, which beat against it. That was refused while a frame the
  pane asked for re-ran the whole window — the first cut took this machine's 240 Hz window from about
  6.5 % of a core to 32.5 % — and the cached regions made it affordable: a notified view marks only its
  own `Part` and the root's shell dirty. Measured with a 16-bit 44.1 kHz FLAC into a 48 kHz null sink
  under a headless KWin over the scratch catalog, the window takes 7.3 % of a core with the pane in
  front. Paused, or with nothing tapped, it asks for nothing: bars still falling ask again on a timer at
  the poll's interval, and a paused transport holds the paused moment. Being an entity keeps its state —
  the transform's tables, the bars, the three sample buffers — out of `RootView`. Every frame is still a
  whole-window paint on the GPU, gpui's and not the pane's. `RootView::render` calls
  `PlayerModel::listen_in` with whether the pane is in front, so the engine taps nothing while it is not.
- **The analysis pane draws the whole of the playing track and says whether it is what it claims.**
  `Pane::Analysis` is the fourth pane under `Section::Playing`, the same six edits, with `Icon::Analysis`
  a waveform over its axis. Its heading is the visualiser's — title and artist through `opens` — reading
  back the codec, declared depth and rate, or how far the decode has got. Under it: the verdict in its
  colour over one sentence per finding, each `Finding::told`; the waveform, a lane per channel drawn as a
  min-max polygon washed in the accent with the RMS solid inside, the part heard shaded and a playhead a
  press seeks from; the spectrogram, the painted raster stretched to the card with its frequencies as
  backed labels and the cutoff as a line in the verdict's colour; the average spectrum as a washed
  polyline over faint lines every 20 dB; the levels and the source as `fields` cards dealt by `beside`,
  lent by the inspector; and the recognition card, led to the top where the audio is not the song the
  file names and carrying *Take this name* there. `analysis_plot.rs` is the arithmetic, with no gpui in
  it. The Online category's *Recognition* group is the `acoustid-key` field, written through the
  `Settings` seam as *Contact* is and read from the next start. `analysis.md` has the model.
- **Listen is a sheet over whatever pane is in front, not a pane of its own.** It names a song that need
  not be in the library, so it belongs to no section: `ctrl-l`, bound in `answering_anywhere`, and the ear
  in the header — stopping its press like every header control — both open it through
  `RootView::open_the_listener`, starting a recording at once; escape or a press outside the card closes
  it and stops one in flight. Stopping is the same in both stages: it drops the task that would ask about
  the clip and puts the sheet back at `Idle`, so a song named after Stop or after closing is neither told
  to the desktop nor drawn. `listening.rs` is the model and `views/listen.rs` the drawing. `ListenModel`
  holds the `Listens` the binary handed in and walks one `Stage` — `Recording` with the `Hearing` the bar
  is drawn from and the source it records, `Asking`, then `Found`, `Unknown`, `Silent`, `Unreached`,
  `Offline` or `NoService` — recording and asking each on the background executor, so the frame never
  waits on PipeWire or a network. `listen` takes whether Online is on *now*, read off
  `ResonateApp::online` by `RootView::listen_now`: off is `Offline` and nothing is recorded, so Online
  switched off in the run stops Listen reaching Shazam though its recognisers were built at the start;
  on with no recogniser registered is `NoService`, which says they come at the next start — Online was
  off when the run began, or the build has no network — rather than claiming Online is off
  (`online_switched_on_in_this_run_is_not_said_to_be_off`). The recording note names the source in the
  stage, not the one chosen, and the source segments and microphone chips take no press while
  `is_listening` — `listen_from` refuses one too — so a press mid-recording neither changes the next
  source nor misnames this one
  (`a_source_pressed_while_recording_is_refused_and_the_recording_names_its_own`). A found song is a card: the cover at `theme::listen_cover()`, title,
  artist, album and year, which service named it, and *Listen again*, *Open* where the service gave a
  link and *Find it* — `search_instead` and `choose_pane(Pane::Tracks)` over title and artist, so a held
  track is one press away and one not held lands on the search listing what the catalog and MusicBrainz
  know of it. Those buttons, and the *Listen* under an idle, unknown, silent or unreached prompt, are
  drawn in `sheet_actions`, a card-wide row starting at the prompt's left edge — `kit::actions` is a
  heading's row, pushed right and held to 64 % of its parent, and in the sheet's column it stood the
  button off at the right of a line the prompt and bar both begin at the left. The last `HEARD_KEPT` (8)
  songs stay listed under it for the run. *Desktop* and *Microphone* are a `kit::segmented` over chips of
  the microphones PipeWire offers, listed each time the sheet opens, a choice written back as
  `listen-from` through the `Settings` seam; Online's *Listening* group writes the same key and
  `listen-for`, and its *Recognition* group carries the AudD token beside the AcoustID key, each a `Field`
  read at the next start. Its *ListenBrainz* group is the `listenbrainz-token` field, not read at the next
  start: the binary's submitter follows the settings file, so a token given or cleared there is what the
  next submission, within half a minute, carries. **A token given is asked about at once.** Where
  Online is on and the build handed `Lookups::scrobblers` — the `Scrobblers` seam turning a token
  into a `Scrobbler`, `None` without the `online` feature — `check_the_listenbrainz_token` asks
  `token_held` on the background executor and says in a toast whose token it is, or that the service
  does not know it, so a token pasted short is found out then rather than by listens never arriving.
- **Play next and Add to queue are row actions, and Shuffle is the tracks heading's second play
  action.** Each track row, the playlists index row and each row of an opened playlist carry the queue
  pair, and on a playlist row they take the whole reach the row is in. The tracks heading carries
  Shuffle beside Play; an opened playlist's heading does too, starting at a time-chosen row and enabling
  queue shuffle. The + on a queue row and each row of an opened playlist opens the playlist picker; the
  index and opened playlist headings use + to enter the target's track-browsing mode. The albums and
  artists panes carry neither, a row there scoping the tracks pane rather than playing, nor does the
  queue pane, its rows being in it already. The two are the only queueing gestures, each named once in
  `menu::PLAY_NEXT` and `menu::ADD_TO_QUEUE`: *Play next* is `Placement::Next`, straight after the track
  playing, and *Add to queue* is `Placement::Queued`, after whatever is queued and ahead of the rest of
  the album or playlist playing. An album page, an artist page and an opened suggestion do not carry the
  pair: their row is under the text. A suggestion's card queues with a mark. The queue pane marks a row
  waiting to play next with the *queue next* icon where its number would be, off `Queued::next`, and its
  heading counts them.
- **The queue is drawn in the four parts the engine keeps it in, each under a heading.** The published
  list is `order[..after] ++ next ++ order[after..]`, so where a row stands relative to the one heard is
  all it is: `QueueParts::of` reads the length, `PlayerState::queue_position` and `Queued::next` into
  runs of `Part::Heard`, `Playing`, `Next` and `Rest` — *HISTORY*, *NOW PLAYING*, *PLAYING NEXT* and
  *CONTINUE PLAYING*, the last naming the playlist where `Library::playing_playlist` still badges one —
  and a queued row being heard is `Playing`, not `Next`, `Queued::next` still spanning it. A heading is
  one more row of the `uniform_list` — an eyebrow, a count and a hairline — so the list keeps one height
  a row; a one-part queue draws none. What was heard is drawn at `HEARD_FADED` and lit whole under the
  pointer. Every gesture still speaks in queue rows, and `QueueParts::line_of` and `line` are the one
  mapping to the list's lines — `show_row` scrolls through it, so following the playing row, the reach
  keys and a type-ahead jump land on the row, not one heading short —
  `every_row_is_shown_at_the_line_that_draws_it`. A row dragged across a heading is sorted by the engine
  as any drop is, by the row it lands after.
- **Every major listing is put in order, from a chip row and from its header.** The tracks, albums and
  artists panes each hold an order and a reading on `LibraryModel::sorting`, kept for the run like the
  settings category and playlists order. Two controls write it: a sort icon opening the ORDER and READS
  chips, and the pressable column header — a press names the order, a second on the one in force turns it
  round, the column in force wearing the accent and a chevron. `views/sorting.rs` holds both: `Ordering`
  is the trait the five order enums implement and `sorting::order_row` the one chip-row builder, what the
  three near-identical builders in `views/playlists.rs` collapsed into. `listing::columns` has a `Sorted`
  descriptor — which columns a listing offers, which is in force, and a `fn` pointer for the press — so
  the tracks pane, the queue and an opened playlist draw one header and cannot disagree about a width
  while offering different vocabularies. A right press on a header offers every order it has.
- **The queue is put in order as a gesture, not a standing order.** Its heading's sort icon sends one
  `Command::Order`: the window builds a permutation over the rows it draws — scanned or read through
  `Player::media` — and the engine rewrites the play order in one pass, the cursor re-seated onto wherever
  the playing row went, so a sort mid-track reopens no stream. A queue row is dragged and reached as a
  playlist row is, and what comes next is still shuffle's and the transport's to say. The heading carries
  the count, total length and row in play, and *Clear* sends one `Command::Remove` over the whole span.
  The reach lives for the run, escape clears it, and one left past the end of a shorter list is pulled
  back to the last row rather than dropped.
- **A heading wraps rather than grinding its name down, and `kit::actions` is the whole of how.** Every
  heading's controls sit in one, so the longest — an opened playlist's Play, Shuffle, *Edit search*, +,
  *Drop shown*, Tidy, Sort, the pin, the ⋯ menu (Play, Play next, Add to queue, Rename, Duplicate,
  Export, Pin, Discard), Undo and Redo — flows onto a second line as the window narrows rather than
  clipping Play off the edge. The actions take at most 64 % of the row and the name keeps
  `theme::heading_name()` of room whatever is beside it, so the controls wrap rather than the name
  grinding to a letter. An album's and an artist's pages are out of this: their actions sit under the
  name.
- **Undo and Redo are drawn in the playlists headings and nowhere else.** `RootView::undo_edit` and
  `redo_edit` are the two, Redo drawn only while a step is there to put back, each saying how many steps
  stand behind the one it offers — `Undoable::behind`, the only number either stack publishes. Rows put
  in a playlist from the tracks or queue pane have no control to press, so the notice names `ctrl-z`
  beside where they landed; the notice a press either way leaves says what was put back or done again,
  not what the playlist now holds.
- **A narrowed playlist says what a gesture reaches rather than leaving it to be found out.** The
  heading's `NARROWED_HINT` says Play, Play next, Add to queue and Drop shown take the rows shown while
  Sort, Tidy and Export take the playlist whole. Tidy removes missing and repeated rows together in one
  undoable edit — there is no separate fold control. The playlist title opens rename, its selector
  appearing on hover. The hint is behind the pointer like every hint, and a search matching nothing draws
  no mark to hover. The sidebar's count is the narrowed one, so standing in a playlist whose name the
  search misses reads 0.
- **A pane hands its rows out by reference count rather than cloning them per frame.** An opened
  playlist's rows are an `Arc<[PlaylistEntry]>` the model shares with the pane, its heading, the list's
  closures and every visible row, and the reach is read off it once a frame as a `Reaching` — the span
  and the cuts it holds — which a row in the reach takes by reference count too. `Held` carries an
  `Arc<[Cut]>` likewise, so the picker's rows are copied only where the press hands them to the library.
  The browse panes read the same way: `LibraryModel` holds albums, artists and tracks as `Arc<[_]>` and
  `albums()`, `artists()` and `tracks()` hand back a pointer, so the tracks pane no longer deep-clones two
  thousand `Track`s — each a `PathBuf` and two `String`s — on each of sixty frames a second. The queue
  pane reads the same way, and a whole list is walked only where a press asks: the playlist menu's
  *Duplicate*, the index row's +, the queue's *Save as a playlist* and the tracks heading's *Add to
  playlist*, *Play next*, *Add to queue* and *Play* each do it inside the listener rather than on every
  frame that may never carry the press — the fresher read as well as the cheaper.
- **The playing row is indexed, not searched for, and resolved once a pane.**
  `PlayerState::queue_position` names the row, so `RootView::queued_row` reads it out of the published
  queue by index and weighs the id it finds against `state.current` rather than walking the queue. One
  `Playing` carries everything the playback bar draws — title, artist, album, codec and a `Cover` of the
  album id and the file the picture comes from — so the panel, cover cell and signal path are one
  resolution rather than three walks and two `Track` clones.
- **A playlist edit re-reads the playlists and nothing else, and so do a scan and a counted play.**
  `LibraryModel::reload_playlists` takes the `ThePlaylists` read above; `reload` re-reads the opened
  playlist beside the listing, which a scan runs every twenty polls and again when it ends and a counted
  play runs once, so `plays:` and `played:` queries move under the window as `added:` does. Inside that
  read the listing is whole, so a rename re-reads the entries beside it: counts and total length are one
  grouped pass over the entries rather than two subqueries a row, and the opened playlist is read once
  more beside them, a narrowed index losing the row the heading draws its name, count and kept order
  from.
- **A second process's edit is seen once its writes settle.** `watch_for_writes_elsewhere` reads
  `Library::written_elsewhere` — the writer connection's `PRAGMA data_version`, typed as a
  `WrittenElsewhere` that is only compared — on a background thread every `WATCHED_EVERY` (2 s), and
  reloads once the stamp has moved since the last reload *and* read the same the tick before, so a
  playlist or favourite `resonate mcp` changed, or a `resonate scan` beside the window, is drawn a few
  seconds after it lands, and a scan committing batch after batch costs one reload when it pauses rather
  than one a tick. This process's own writes never move the stamp, so the window's own edits are the
  reloads they always were. A busy writer answers nothing and the tick is passed over.
- **`Raise` activates the window and `Quit` closes it.** The binary's host answers `can_raise` with
  whether it holds a raise channel — the window's does, windowless `resonate play` and the bus's default
  `Host` answer false — and `Raise` sends on it; `raise_when_asked` drains it on the gpui foreground and
  calls `activate_window` on every window, which a Wayland compositor is still free to refuse without an
  activation token. `Quit` sends on the same channel `resonate play` uses, drained by the gpui foreground
  on the frame timer.
