---
paths:
  - "crates/resonate-ui/**/*.rs"
---

# The window

`resonate-ui` is GPUI on Wayland. It depends on the engine, the library, the lyrics and `resonate-listen`, and on
neither `resonate-codec` nor any image crate; a `Reference` reaches it only as the trait object
the binary hands `run` inside `Lookups`, so it never names the online crate either.

## Chrome and input

- **The window is its own titlebar.** `WindowOptions` asks for `WindowDecorations::Client`, and the
  header carrying the Resonate wordmark and the search field draws the minimise, maximise and close
  controls beside them, drags the window and opens the compositor's window menu. The header is
  `theme::header_height()` tall and three columns: the wordmark and the window controls each take the
  sidebar's width less its padding, so the search field between them centres on the window rather
  than on what the wordmark leaves, and it grows no wider than `theme::search_width()`. An empty,
  unfocused field draws `ctrl-f` in the mono face at its end, because the one key that takes focus
  deliberately is otherwise nowhere on screen. `views/chrome.rs`
  owns the frame around it: a `theme::RESIZE_BORDER` gutter outside the painted window carrying the
  resize edges and the shadow, and rounded corners on whichever edges are not tiled.
  **gpui clips a child to its parent's rectangle, not to its rounding**, so the header and the
  playback bar, whose `surface` fill reaches the window's corners, painted a square of it past the
  rounded border. `chrome::rounded` is the one reading of which corners are round and
  `rounded_within_the_frame` the same corners less `FRAME_BORDER`, which the header takes on its
  top corners and the playback bar on its bottom ones, so the fill nests inside the border. **An edge held
  to the screen is not a resize edge.** A tiled edge loses its gutter, so the content runs to it,
  and `grabbed_edge` reading the outer `RESIZE_BORDER` of every edge regardless turned a press on
  the bottom strip of a maximised window's playback bar into a compositor resize grab that ate the
  click. It weighs only the edges `held_edges` leaves free — none at all while the window is
  maximised or fullscreen, whatever tiling the compositor reported — which is the rule Zed's frame
  follows too, and `chrome.rs`'s tests hold it. A compositor
  may refuse the request and gpui then reports `Decorations::Server`, so the controls and the drag
  handlers are rendered only under `Decorations::Client` — a window that kept a server titlebar must
  not grow a second set of buttons. **Which of minimise and maximise are drawn is two readings
  weighed together**: `Window::window_controls`, which is what the compositor offers, and
  `ResonateApp::window_buttons`, a `WindowButtons` the `minimise-button` and `maximise-button` keys
  fill and the Appearance category's *Window buttons* switches write. `RootView::window_controls`
  draws a button only where both say yes, so a setting can take one away and never conjure one a
  compositor withheld. The switch writes the global and notifies the root view, so the header is
  redrawn on the next frame rather than on the next run. Close is not a setting — it is the one way
  out a pointer has — and a double press on the bar goes on zooming whether the maximise button is
  drawn or not, because `chrome::titlebar` never asked it. gpui forces every edge tiled when the
  window is maximised or fullscreen, which is what drops the gutter, the rounding and the shadow
  there. `window_min_size`
  is the point the header's own children stop clipping rather than a size the panes were designed
  around, and a compositor is free to ignore it. **None of the four marks is a glyph.** The minimise
  mark is a bar `div`, the maximise and restore marks are bordered `div`s, and the close mark is
  `Icon::WindowClose` — an SVG of its own, whose × spans the whole viewBox where `Icon::Close` is
  inset in its box, so it renders at the square's own edge. All four are `theme::WINDOW_MARK` less
  its overlap across, which is what puts the three at one size on one centre line. A glyph could
  not: no box is in the primary UI face, so the maximise mark always had to be drawn, and − and ×
  centre on the math axis rather than on the box, which left the × half a pixel off the square
  beside it and at half its width.
- **Closing takes the window away and quitting follows it, never the other way round.** The close
  control, `ctrl-q` and the bus's `Quit` all call `Window::remove_window`, and `app.rs` quits once
  the last window has closed. Calling `App::quit` under a live window cleared gpui's window map
  while the compositor still held the surface, so the pointer leaving the close button it had just
  pressed arrived at a window that was no longer there and gpui logged *window not found* twice on
  every exit. Removing the window first drops the platform window and its callbacks with it, and
  Wayland's own client stops the loop when its last window goes.
- **The window names itself to the compositor twice over.** `WindowOptions::app_id` carries
  `APP_ID`, which is what `xdg_toplevel.set_app_id` sends and what a compositor matches against
  `resonate.desktop` for the taskbar icon and for window grouping; the entry's own `StartupWMClass`
  is held to the same string by a test in `app.rs`. The caption is a second call, because gpui's
  Wayland backend never reads `WindowParams::titlebar` — `TitlebarOptions::title` names the header
  this window paints itself and nothing the compositor sees, so `Window::set_window_title` on the
  open is what puts a name in the taskbar and the window switcher.
- **The header is a titlebar, so everything clickable inside it stops the press.**
  `chrome::titlebar` puts `start_window_move` on the whole bar, and a mouse-down that reaches it is
  a drag rather than a click — which is why the window controls and the search field both carry
  `on_mouse_down(Left, stop_propagation)`. A control added to the header without one is unclickable,
  not merely unstyled.
- **The search field is a real text input, not a keystroke accumulator.** `views/field.rs` owns the
  `Field` entity and `edit.rs` the `Edit` behind it — the content, the caret and the selection, with
  no gpui in it, which is what lets the caret, the word motions and the grapheme steps be tested
  without a window. The view half is a custom `Element`: it shapes the line, paints the selection
  and the caret, and registers an `ElementInputHandler`, which is what gives the field IME
  preediting, a `bounds_for_range` the candidate window is placed against, and a caret that maps to
  and from a pointer position. The line scrolls under the field to keep the caret in view rather
  than truncating, and a selection drag is followed from a `Window::on_mouse_event` registered in
  `paint`, because the element's own `on_mouse_move` fires only while the pointer is over it — the
  same reason `views/slider.rs` carries a drag surface.
- **The search field's undo step is a span of edits, not a keystroke.** `edit.rs` keeps a bounded
  stack of content-and-selection snapshots and coalesces consecutive edits that carry the same
  `edit::Span` — `Typing`, `Removing` or `Composing`, distinct from `resonate-core::Span` — *and*
  resume from the selection the last one left, so a typed word is one step, a run of backspaces is
  one, and an input method revising a preedit is one however often it revises. A paste, a cut and a
  clear are `Span::Discrete` and never join what came before, which is what makes `ctrl-a` over the
  lot, and escape, recoverable. Whitespace closes a typing run, so undo walks back a word at a time.
  The history belongs to `Edit` rather than to the view, so it is tested without a window like the
  rest of it. It holds 128 steps and lives only as long as the run, and escape clears the field and
  blurs it in one stroke, so what it threw away is out of reach until a click puts the caret back:
  the three keys are bound under `SEARCH_CONTEXT` and a blurred field never sees any of them.
  Outside the field the same three walk a playlist edit instead, so what they reach depends on where
  the caret is.
- **The window names itself, and a negated binding is dead without it.** The root div carries
  `key_context(WINDOW_CONTEXT)` beside its `track_focus`, so the context stack is `["Resonate"]` at
  rest and `["Resonate", "Search"]` with the caret in a field. It is not decoration: gpui's
  `KeyBindingContextPredicate::depth_of` returns `false` for an *empty* slice before it ever reaches
  the `Not` arm, so a window that names nothing disables every `!Search` binding — the transport
  keys, the reach keys, undo and redo alike — while the field's own bindings keep working, because
  the field is the one element that did name itself. The asymmetry is the tell. `app.rs`'s tests pin
  both stacks against the two predicates, and one of them asserts gpui still refuses the empty
  slice, so a toolchain that changes that answer says so rather than quietly making the context
  pointless.
- **An album or an artist opened stands under its category, and pressing the category again
  leaves it.** The page is `Pane::Tracks` narrowed to a `Selection`, but a listener who opened it
  from the albums grid is *in Albums*, so `RootView::in_front` — `in_front_of`, arithmetic over
  the pane and the selection — is what the sidebar marks: Albums for an album, Artists for an
  artist, the pane itself otherwise. A sidebar press is `RootView::choose_pane`, and `landing` is
  the whole of the decision, tested without a window: the row already in front goes back to the
  whole of its category, clearing the scope or closing an opened playlist; a category whose page
  is still standing behind another pane goes back to that page, the way a tab keeps its place;
  *Tracks* always lists every track, because a scope never belongs to it; anything else is only a
  change of pane. Before, the sidebar marked *Tracks* while an album was open, pressing *Albums*
  left the scope standing to surface later under *Tracks*, and pressing *Playlists* inside a
  playlist did nothing at all.
- **`ctrl-tab` walks the sidebar and the browse panes answer the reach keys.** `NextPane` and
  `PreviousPane` step `Pane::BROWSE` from the pane `in_front` names, through `choose_pane`, and
  wrap; a pane the sidebar does not list steps onto the
  first one it does, and `stepped_pane` is that arithmetic, tested without a window like
  `Step::landing`. `Shift::Listing` names the tracks pane or the artists pane, each of which grew
  a `UniformListScrollHandle` so `show_row`'s centred scroll works there, and a reached row wears
  the same accent edge a queue row does. What a listing refuses is what it has no order of its own
  to change: `Shift::is_edited` is the reading, so `delete`, `alt-up` and `alt-down` are dead there
  while the reach, the page keys and `enter` are not — `enter` plays a track or opens an artist,
  which is what a click already does. `ctrl-shift-left` and `ctrl-shift-right` join the media
  keys in `answering_anywhere`, so one pair always steps the queue, caret or no caret.
- **A search is left for its results from the keyboard, and the albums grid is reached too.**
  With the caret in the search box, `down`, `tab` and `enter` are `GoToTheResults`, `TabOnward`
  and the field's own `Submitted`, and all three land on `RootView::go_to_the_results`: the caret
  goes back to the window and the first row of whatever the pane lists is reached, so the next
  `down` steps through what was found and `enter` plays it or opens it. `tab` is bound under
  `SEARCH_CONTEXT` so that it outranks the window's own, and from any field but the search box
  `tab_onward` is the `focus_next` it always was. A query that moves drops a reach left in a
  listing, because the rows under it are no longer the ones it was put on, so `down` after typing
  starts at the top. `Listed::Albums` makes the grid a listing the reach keys answer: they step
  through the albums in reading order, a page is as many whole rows of the grid as the pane
  shows, `show_row` scrolls the grid row holding the album, `enter` opens it, and the reached
  cell wears `reached_ring` — an accent border laid over its cover, which costs the grid no room.
  `down_and_tab_in_a_field_are_the_fields_own_before_they_are_the_windows` pins the two bindings.
- **The window binds the media keys too, as a fallback rather than as the feature.** A desktop that
  grabs the transport keys consumes them and calls MPRIS, which is how they are meant to work and
  what makes them work with the window behind everything else; where a session grabs none, the key
  reaches the focused window instead and the binding is what answers. gpui names an unmapped keysym
  by lowercasing `xkb_keysym_get_name`, so they are bound as `xf86audioplay` — the one play/pause
  key most keyboards carry, toggling — `xf86audiopause` to `Pause`, `xf86audiostop` to `Stop`,
  `xf86audionext` and `xf86audioprev`, and `XF86Back` and `XF86Forward` are two gpui does map, to
  `back` and `forward`. All seven are in `answering_anywhere`, because a key on the keyboard's own
  transport row should not stop working because the caret is in the search field. `app.rs`'s test
  pins the seven names against `Keystroke::parse`, since a name gpui cannot read is a binding bound
  to nothing rather than an error.
- **Typing at a list that no search narrows jumps through it instead.** The global search narrows
  the tracks, albums, artists, playlists-index and opened-playlist panes, so typing at any of
  them is exactly what a listener means. The queue narrows on nothing, so `RootView::typed` routes
  a keystroke there to `TypeAhead` before the search arm: `jumped` prefers a row that *starts
  with* the letters over one that merely holds them and wraps at the end, `RootView::jumping` is
  the one place that says which panes opt in and knows how to read their rows, and the landing is
  the `reach_at` and centred `show_row` the reach keys already make. The pill says **JUMP TO** or
  **NO MATCH** in words rather than in a colour, because a colour cannot be read in a screenshot
  and a failed jump has to be legible; it has no id and no listeners, so gpui inserts no hitbox
  and it blocks nothing under it. `typing.rs` holds the arithmetic with no gpui in it and is
  tested the way `Following` and `Step::landing` are. It folds through
  `resonate_library::folded_letters`, the same fold the search box and `artists.key` read, so
  *przybylowicz* reaches *Przybyłowicz* at the queue as it does in the search field.
- **Backspace drops a typed letter before it drops a reach.** A jump *sets* the reach, so the
  other order would have made backspacing a mistyped letter in the queue take the row the jump
  had just landed on out of it. `drop_typed` answers false whenever nothing is live, so with no
  pill up the key does exactly what it always did. Escape clears a live type-ahead after the
  magnified cover and the notice and before `dismiss_search`: those two stand until they are
  taken down while this one takes itself down after a second, and somebody abandoning a jump has
  not asked for their search to be cleared.
- **Typing anywhere still searches, and that is why it does not take focus.** `RootView::typed`
  appends to the field without focusing it, so `space` stays play/pause until a click puts the caret
  in. `space`, `s`, `h` and `r` are bound under `!SEARCH_CONTEXT`, and so are `left`, `right`,
  `ctrl-left` and `ctrl-right`, which the transport would otherwise take from the caret; so are the
  reach, move, undo and redo keys. An action fires before any `on_key_down` listener and stops
  propagation, and a keystroke that reaches neither is what the platform hands to the input handler,
  so a key bound with no predicate could never reach the query at all. **Which predicate a key
  carries is structural rather than per-line**: `answering_anywhere`, `answering_away_from_a_field`
  and `answering_where_the_caret_is` are three lists and `bindings` is their concatenation with
  `field::bindings`, so a new key is put in one of them and
  `every_key_not_named_to_answer_anywhere_is_dead_while_a_caret_is_held` walks the second and holds
  every one of it to a predicate that is dead in a field and on a control and alive at rest. That
  test is what `up` and `down` were missing: they carried volume with no predicate at all, so the
  volume moved while a name was being typed, and it is also why volume is now `ctrl-up` and
  `ctrl-down` — the plain arrows are the motion a hand reaches for on a list, and a list is what
  they now move. The editing keys live in `field::bindings` under the same context. Escape blurs and clears, and closes the
  naming row before it clears a search; a press anywhere outside blurs through `on_mouse_down_out`.
  `ctrl-f` is the one key that takes focus deliberately, which is why it is bound with no predicate:
  it reaches the field from inside it too, and it selects what is there so the next keystroke
  replaces the query rather than extending it.
- **Escape away from the field is a chain, and a notice is in it.** `RootView::typed` is what hears
  escape outside the search field, because `LeaveSearch` is bound under `SEARCH_CONTEXT` alone: an
  action fires before any `on_key_down` listener and stops propagation, so binding escape under
  `!Search` as well would take the key off the handler that already answers it. The order is the
  match arms — a magnified cover shrinks first, then a notice the engine raised or a settings write
  could not make is taken down through `RootView::dismiss_notice`, and only then does
  `dismiss_search` close the naming row, clear the query, or — where the query is already empty —
  step back through `RootView::step_back`, which is the same seam the page's way back presses.
  That last step is what makes a scope escapable: opening an artist from the tracks pane
  leaves the pane where it was and narrows it, so without a key the only way back was a button in
  the heading. The chain backs out innermost first, so a scoped search takes two presses — the
  words, then the scope. `dismiss_notice` is the one seam and the
  playback bar's strip presses it too, which is why `DISMISS_HINT` reads *click or escape to
  dismiss*. The caret keeps escape for itself: a focused field never reaches the match at all,
  because `editing` returns before it, so what escape does in the box is what it always did and a
  red line that only a track change used to clear now has a key.
- **A slider is grabbed on the press and followed from a window-wide surface, not from the rail.**
  `views/slider.rs` owns both rails: the press starts a `Grab`, and `drag_surface` — an absolutely
  positioned child of the app that occludes while held — carries the move and release listeners,
  because a rail's own `on_mouse_move` fires only while the pointer is over those few pixels. The
  surface has to be a *child*: `chrome::frame` adds its own `stop_propagation` move listener to the
  app element itself, and listeners on one element run in reverse order, so anything registered
  beside it in `render` would never see a move. Volume applies on every move and the `VOLUME_SETTLE`
  debounce is what keeps a drag from rewriting `config.toml` once per pixel; a seek only previews,
  and commits one `Command::Seek` on release, so a scrub costs one gap rather than one per pixel.
  The wheel over the volume cluster is a notch of `VOLUME_A_NOTCH` — a touchpad's pixels counted
  in `PIXELS_A_NOTCH` — while `ResonateApp::scroll_volume` says so, and `RootView::volume_aimed`
  is what a run of notches or held keys adds to: the engine publishes the new volume a poll
  later, so reading it back each notch lost every notch but one inside a poll.
  **A press on the speaker mutes, and mute is not a volume.** `RootView::toggle_mute` sends
  `Volume::MUTE` and keeps the level it was heard at in `muted_from` without storing anything, so a
  run quit while muted opens at the level the listener chose rather than at nothing. `muted_at` reads
  the pair only while the engine still publishes nothing, so a volume set over the bus ends the mute;
  a second press, a notch of the wheel, `ctrl-up` and `ctrl-down` all start again from the kept
  level, and a grab of the rail is a volume of its own and forgets it.
  The release is taken from three places — a release inside the window, `on_mouse_up_out` for one
  outside it, and a move that reports no button held — because a pointer that leaves the window
  mid-drag is otherwise never told to let go. The equaliser's handles ride the same surface: a
  `HeldBand` beside the `Grab` makes it occlude and follow, and one release lets either go —
  `eq.md` has the curve's side of it.
- **A drag that leaves the window stops being followed, and that is the compositor's line.** gpui
  reports motion only while the pointer is inside the window, so a slider carried past the edge
  freezes at the last value inside it and lets go on the way back rather than tracking to the end of
  the rail, and a text selection drag tracks anywhere inside the window and stops at its edge.
  `on_mouse_up_out` still hears the release; nothing hears the motion.
- **`PlayerModel::state` hands back a borrow of the model, not a copy.** It borrows `cx` for as long
  as the reference lives, so a pane reads the model once and clones what it needs —
  `let model = self.player.read(cx); let state = model.state().clone();` — before anything taking a
  `&mut Context`. Writing `self.player.read(cx).state()` inline and then calling `now_playing` or an
  entity's `update` is a borrow error rather than a matter of taste.

## The design system

- **`theme.rs` is the palette and the scale, and `views/kit.rs` is the vocabulary every pane is
  built from.** A palette is a `Flavour`: the surfaces `background`, `surface`, `raised` and
  `hover`, the `border` and `outline` between them, `text`, `muted` and `faint` over them, the
  `pitch` and `paper` either of which may be what is written on an accent fill, the `scrim` a sheet
  is laid under, the `alarm` the close control turns, seven `Accents` and the `native` one of the
  seven the palette was built around. Which flavour, which of its accents and which `TextSize` is
  worn is a `resonate_core::Appearance`, whose `accent` is an `Option<Accent>` — `None` being the
  palette's own, resolved by `Flavour::accent` and by nothing else — and `theme::wear` is
  the only way to set one. Every colour is read back through a function of its own —
  `theme::accent()`, `theme::background()` — so a pane names no constant and a theme change is one
  write behind one lock rather than a value baked into every call site. **Every measure that holds
  text is read back the same way**, and for the same reason: `theme::row_height()` and
  `theme::text_base()` are `scaled` off the worn `TextSize`, so one setting moves the type ramp, the
  rows, the sidebar, the header and every control together and nothing is left behind at its old
  size. What stays a `const` is what belongs to the compositor rather than to the type —
  `CORNER_RADIUS`, `RESIZE_BORDER`, the window marks, the rail's track and thumb — and
  `WINDOW_MIN_*`, which is read once when the window opens. **What is written on an accent is
  chosen by contrast, not by a lightness threshold.** `theme::ink_over` weighs the flavour's `pitch`
  against its `paper` and answers whichever reads better on the colour it is given, so a generated
  palette needs no hand-picked ink and a deep accent is written in paper where a pale one is written
  in pitch. It takes a colour rather than reading the worn accent, because `kit::dot_swatch` draws
  its check on an accent that is not worn yet. The semantic colours come off the worn accents rather than a second list: `bit_perfect`,
  `lossless` and `done` are its green, `repacked` its blue, `dithered` its amber, `failure` its red,
  and `converted` and `lossy` are the muted grey, so a colour means the same thing in the playback
  bar's signal path, the inspector's stages, a row's format badge and a device's badge whichever
  palette is worn. `theme::tinted` is how a colour becomes a wash — a selected chip, the playing
  row, a badge's ground — so no second palette of washes has to be kept in step, and
  `theme::selection` is the accent worn thin rather than a colour of its own.
- **Every theme is dark, and a test says so rather than an eye.** Sixteen of them: Resonate's own
  warm near-black, Midnight, Graphite, AMOLED and Plum, Rosé Pine and Rosé Pine Moon, Catppuccin's
  Mocha, Macchiato and Frappé, Nord, Gruvbox Dark, Tokyo Night, Dracula, One Dark and Solarized
  Dark — each carrying the same seven accents,
  Mauve, Blue, Teal, Green, Amber, Peach and Red, under its own hex, so an accent is part of a
  palette rather than laid over
  it. **What a fresh install wears is the palette's own accent rather than a colour named once for
  all of them.** Every `Flavour` declares a `native` accent — Nord's frost blue, Gruvbox's amber,
  Catppuccin's and Rosé Pine's mauve — `Appearance::DEFAULT` names none, and the Colour card offers
  it as a ringed swatch ahead of the seven, so switching palette moves the accent with it until one
  is chosen by hand. Choosing none again is `Setting::Accent(None)`, which takes the `accent` key
  out of the file the way a cleared contact does, rather than writing a colour. Sixteen is also
  what fills the shelf: the theme cards are `theme_swatch()` wide in a `settings_column()` body,
  which is four to a row, so a palette is added four at a time and every row stays full.
  **AMOLED is the one whose panes are pure black**, `background` and `surface` alike, so the pixels
  behind the lists are off on an OLED panel; what is raised and hovered still steps up from it,
  and it wears Resonate's own accents. **Where a published palette names no hue for an accent,
  its own terminal mapping decides**: Dracula has no blue, and its ANSI blue is its purple and its
  magenta its pink, so those are its Blue and Mauve and it is built around the purple. One Dark's
  `abb2bf` reads at 6.6:1 on its ground, so its body text is the palette's highlighted `d7dae0`
  and `abb2bf` is what is muted; Solarized's `base0` reads at 4.8:1, so its text is `base2` and
  its muted `base1`, and its violet, orange and red carry their ink at 4.5:1 only on a pitch of
  pure black, which is what its pitch is rather than a lifted accent.
  `theme.rs`'s tests walk every theme
  crossed with every accent — and with none — and hold the contrast to the recognised bars: 7:1 for text on the panes
  and the cards, 4.5:1 for text on what is raised above them, for muted text and for what is written
  on an accent fill, and 3:1 for the accent against the panes. Nord's own aurora red is the one
  colour that could not carry its ink at 4.5:1 and is drawn a fifth of the way towards `nord6` so
  that it can — which is what the guard is for, rather than trusting a palette because it is
  famous.
- **A palette is either quoted or ramped, and hand-writing twenty is how one drifts.**
  A quoted `Flavour` is a published palette's own hexes, written out as a `static`, which is what
  keeps Catppuccin's Mocha Catppuccin's; a ramped one is `ramp` over a `Recipe` — one hue, one
  chroma — run through a fixed
  lightness ladder that places the ground, what sits below it, what is raised above it, the hover,
  the hairline, the drawn edge and the three inks, and then generates all seven accents at the
  canonical hues. Midnight, Graphite and Plum are three numbers each rather than twenty colours, and
  Graphite is Midnight at a chroma of nothing. A ramped flavour is a `LazyLock` rather than a
  `const`, because turning an HSL triple into a packed `u32` is arithmetic a `const` context will
  not run; `hsl` is held to the six corners of the cube by a test, so the ladder cannot quietly
  shift under the palettes it builds. The three share one set of accents, because an accent is
  what a colour *means* here — green is bit-perfect wherever it is drawn — and a palette's own
  character is its surfaces.
- **Two faces, and figures are always in the second.** The window names Inter for text and
  JetBrains Mono for anything a listener would compare across rows — clocks, rates, counts, bitrates,
  kbps, the tag values in the inspector — and `theme::mono` builds the `Font` with `tnum` on, so
  figures line up down a column. `kit::figure` is the one way to draw one. Neither face is embedded,
  so **which family is drawn in is settled once against what the machine actually has**: `fonts.rs`
  holds a ladder per face and `fonts::settle` walks it against `cx.text_system().all_font_names()`
  as the application starts, answering with the installed spelling, so a machine without Inter draws
  Adwaita Sans or Cantarell rather than whatever fontconfig happens to resolve. Neither ladder
  offers a family the other one does, which a test holds, so a body face can never be taken for a
  mono one; a machine with none of them keeps the family that was wanted and lets gpui fall back.
  It is not a setting and has no key — a key with no control is the thing the settings rebuild
  went looking for — and the About card reports which two it settled on. `theme::text_xs()` through
  `text_title()` and `text_lyric()` are the whole type scale, set through `text_size` rather than
  gpui's rem steps, so the root's `text_size(theme::text_base())` is what a bare `div` inherits.
- **A control is a `kit::button` in one of three tones, and nothing draws its own.** `Tone::Primary`
  is the accent fill and is the one gesture a heading leads with — *Play* — `Tone::Outlined` is the
  raised secondary — *Add to queue*, *Save as a playlist*, *Show graph* — and `Tone::Ghost` is
  everything else. Every one carries an icon slot and a hint. `kit::icon_button` is the square
  version a row's controls and the playlist index's actions use, `kit::chip` is the pill an order,
  a reading, a cap or a setting is chosen from, and `kit::badge` is the small mono tag a codec, a
  playlist's kind or a device's standing is drawn as. `kit::format_badge` is the codec badge beside
  the depth and rate spelt out by `format::quality` — *24-bit / 96 kHz*, never the `24/96`
  shorthand, which read as a fraction to anyone who had not met it — coloured by
  `Codec::is_lossless`. The row's format cell is `theme::row_format()` wide because
  *32-bit float / 44.1 kHz* beside its badge is the longest thing it holds.
- **Whether a control can be pressed is the builder's decision, and a greyed one carries no press at
  all.** `kit::Press` is `Takes` or `Greyed`, `kit::button_when` and `kit::mark_when` are the two
  builders that read it, and `kit::button` and `kit::icon_button` are those two under `Press::Takes`
  — so the thirty-odd ordinary call sites are unchanged. A `Greyed` control is drawn at
  `kit::GREYED`, takes `cursor_default`, and is given no hover style, no pointer cursor and no
  `icons::lit_on_hover`, so nothing under the pointer says it would answer. It has to be decided
  where the element is built rather than bolted on afterwards: gpui's `hover` carries a
  `debug_assert!(hover_style.is_none())`, so a second call panics a debug build and a control cannot
  have its hover painted on and then taken off again. `settings::action` is where the pane spends
  it — it takes the builder and the press *together*, hands the builder a `Press` and attaches the
  listener and the focus ring only on the `Takes` branch, so `action` (*Add folder…*, *Rescan folders*, *Stop*) and
  the folder row's forget mark all give it their listener and no call site can hang a press on a
  control the pane has greyed out. What greys them is `LibraryModel::is_busy`, the same answer
  `LibraryModel::scan` refuses on, and not `is_scanning`, which is false throughout a forget:
  *Add folder…* used to be pressable there, open the XDG portal, and have the folder it came back
  with dropped without a word. A forget mark is keyed by the root it stands for — an
  `ElementId::Path` under a `"forget"` child — rather than by the one id every row shared. An
  enrichment is deliberately not a `Work`: `LibraryModel::enriching` is a slot of its own beside
  it, so a lookup running greys the Online card's *Look up* and *Refresh all* and the Library
  card's *Enrich* through `held_back`, and a scan can still be started under one. The run is
  visible from every pane: `RootView::enrichment_status` sits above the sidebar's *Settings* row
  only while `LibraryModel::is_enriching`, drawing the globe in the accent, *Enriching…* and the
  albums and artists asked so far off `enrich_stats`, and a press on it opens `Category::Online`.
  It says albums and artists alone because the sidebar is `SIDEBAR_WIDTH` and the full
  `asked_so_far` line does not fit at `TEXT_XS`.
- **What a listener reaches for is what the lookup asks about next.** `LibraryModel` holds one
  `Sought` for the whole run and hands it to every `enrich`, so a nudge made while nothing is
  running is still there when a run starts. Three gestures put something on it, all through
  `LibraryModel::ask_about`: scoping the tracks pane to an album, which names the album and its
  owner off `album_owner`; scoping it to an artist; and playing a row, which names the started
  track's `album_id` and `artist_id`. It only ever reorders what the pass has left — an album asked
  yesterday is not asked again by a click — so it is a nudge rather than a command, and
  `library.md` has how the pass reads it.
- **Every pane opens with the same heading, and a page inside a category opens with a hero.**
  `kit::heading` holds a `kit::heading_row` — an eyebrow naming the section (*LIBRARY*,
  *COLLECTION*, *NOW PLAYING*, *SETTINGS*), a `kit::title`, a `kit::subtitle` carrying the counts
  and the total length, and `kit::actions` on the right — and, under it, the *Reads* chips, a
  naming row and a notice where a pane has one. The actions wrap inside at most 64 % of the row,
  which is what keeps a long playlist name from being ground down to a letter by thirteen
  controls. **An album or an artist is not a pane heading with a picture bolted on**: it is
  `album_page_heading` and `artist_page_heading`, a `kit::way_back` at the top left and a
  `kit::hero` under it — the cover or the portrait at `theme::scope_cover()`, then a column of the
  eyebrow, a `kit::hero_title` at `text_title`, the lines about it and `kit::hero_actions` *under*
  the text rather than beside it. The actions used to sit on the right of the row and took the
  room the name needed, so an album title was clipped to a few words at `text_xl` while six
  buttons stood beside it; under the text the title has the whole width and wraps rather than
  clipping. **The hero's column is measured, because what wraps in it has to be told its width.**
  `kit::measures_its_width` is a canvas writing `RootView::hero_width` a frame behind — the shape
  the album grid's `grid_width` takes, and that grid now uses the same builder — and the title,
  the action row and the genre tags take it through `kit::hero_title` and `kit::wraps_within`. A
  flex item whose height depends on how its text or its children wrap is measured by taffy at
  the column's *min-content* width, so without a pixel the action row was measured one button to
  a line and the heading stood some 150 px taller than what it drew. Until the first frame has
  measured, the title is one line ending in an ellipsis. The opened playlist's heading carries
  the same `kit::way_back`, reading *Playlists*, above an eyebrow of *PLAYLIST* or *SAVED SEARCH*.
  **A title that is a link is `kit::linked_title`, never `kit::title` over an `opens`.** The lyrics,
  inspector, visualiser and analysis headings name the playing track through `opens`, and under
  `kit::title`'s block box the link was laid out at no width at all, so the heading drew its
  ellipsis and nothing else; `linked_title` is the same face as a flex row holding the link
  `keeps_its_width`, the shape the by-line's artist already had.
- **A track row is the same eight cells wherever it is drawn.** `listing::columns` is the column
  header the tracks pane, the queue and an opened playlist all put over their rows, and
  `number_cell`, `title_cell`, `artist_cell`, `format_cell`, `heard` and `length_cell` are the cells
  under it, so no two of them can disagree about a width. The title and the artist are the two
  that give way, and they give way together: `title_room` and `artist_room` both start from
  `theme::row_artist()` and both shrink, and only the title grows. A title that was `flex_1` from
  nothing beside an artist of fixed width was the first thing a narrow window took, to the last
  letter, while the artist kept all two hundred pixels. The playing row draws
  `listing::playing_mark` in the number cell and its title in the accent. A row's controls sit in
  `browser::row_controls`, which is invisible until the row is hovered — the heading carries the
  same gestures for the whole listing, so a row does not have to advertise its own.
- **Whatever a gesture takes out of the queue is kept, and the queue says so.** A playlist edit
  answers with a `Notice` and an undo; the queue answered with rows that were simply gone, which is
  the same gesture with none of the safety. `TakenOut` is what the last one took — the rows, the
  row they started at and the ids the queue was left holding — and `RootView::took_out` is the one
  the offer stands on. `drop_rows` is where it is written, so a row's ✕, a reach of thirty rows
  and the backspace key all keep what they take; *Clear* is the same call over `Span::between(0,
  last)` rather than a `Command::Remove` of its own. The heading draws `listing::noticed` for it —
  *Took 34 tracks out of the queue* — and *Put back* sends the rows as a `Command::Insert` at
  `Placement::At` the row they came out of. The rows are kept as `QueueItem`s rather than as ids,
  because `one_id_each` mints new ones on the way back in.
- **A run of gestures is walked back through one at a time, and each step is the queue the one
  before it left.** `TakenBack` is the bounded stack `TakenOut` now sits in: *Put back* pops the
  top, and the step beneath it then stands, because putting rows back restores exactly the queue
  the earlier gesture had left — `Unclaimed::claim` hands a removed row its own id back, so the
  ids `stands_over` weighs are the ones it was written against. A gesture whose queue no longer
  stands clears the whole walk rather than leaving stale steps under a fresh one, which is why
  `keeping` is handed the queue *as it was before the drop*: the previous top standing over that
  queue is what says the two gestures are consecutive. `KEPT_GESTURES` bounds it at sixteen, the
  oldest going first, and the heading says how many are behind — the notice counts them and the
  button's hint names the next one — so a walk is as visible as the playlists pane's `Undoable`
  makes its own.
- **The offer stands while the queue is what the gesture left, and `TakenOut::stands_over` is that
  reading.** It weighs the ids the queue holds now against the ids it held once the rows were out,
  so anything that queues, loads, moves or takes away a row takes the offer with it — the row the
  rows came out of is a place in a list that no longer exists, and putting them back there would
  be a guess. It is why `queue_pane` no longer returns `empty` outright: an emptied queue is the
  one place the offer could be seen, so the heading is drawn over the empty pane for exactly as
  long as it stands, while *Clear* and *Save as a playlist* are hidden there, neither having
  anything to act on.
- **A card leaves out what the file does not declare, and the cards stack in two columns.**
  `fields` takes an `Option` per row and draws only what is there, falling back to one faint line
  where a whole card is empty — the shape `tags` always had, now shared by all five, so a file
  with no ReplayGain no longer renders five rows of the literal *unknown*. The five were `flex_1`
  in a `flex_wrap`, which stretched every card in a row to the tallest in it and left the last row
  ragged; they are dealt into two `flex_col` columns that stack tightly and still collapse to one
  in a narrow window.
- **The inspector is the one pane that reads the catalog rather than the engine, and the *Heard*
  card is why.** Every other card on it is the `StreamDigest` or the `OutputStatus` — what the
  decoder and the sink say about the track playing now — but how often a track has been played and
  when it was last played are the catalog's, and nothing else on screen says either. The reading
  comes off the `Track` that `Playing` already resolves through `LibraryModel::track_of`, so the
  card costs no query of its own: `Heard` is three fields carried on `Playing`, and a row no scan
  has seen answers *no library row* rather than zero, which is a different thing. `format::since`
  is how the two dates read — the largest span that fits, *3 days ago* rather than a stamp — so
  the card says what a listener wants to know without a calendar.
- **What a search matched is lit where it is drawn, and one element does it.**
  `listing::matched` answers the text itself where nothing is lit and a `StyledText` carrying
  `HighlightStyle` runs where something is, so an unsearched row costs no more than it did and a
  lit one still truncates, wraps and measures as one string rather than as a row of spans. The runs
  are `Search::lit` for that cell's `Column` — see `library.md` — which is why a name reached by a
  fold lights the spelling it is drawn in: typing *przybylowicz* lights *Przybyłowicz*. The browse
  panes are the only callers, because they are the only listings a search narrows: a track row's
  title and artist, an album cell's title and artist, an artist row's name. The queue and an opened
  playlist pass no runs. The accent is what lights a run, the same colour the playing row's title
  wears, with `FontWeight::MEDIUM` behind it so a run still reads where the whole title is already
  accented.
- **The albums pane is a grid, and its column count is measured a frame behind.** A cell is
  `theme::grid_cover()` square with the title and the artist and year under it, and a row of the
  `uniform_list` is `columns` of them, so a library of two thousand albums still costs a screen of
  cells a frame. The width the columns are read from is `RootView::grid_width`, a cell a canvas
  under the list writes on prepaint; when it moves the canvas asks for the next animation frame,
  because a refresh asked for mid-draw is ignored. **The list is not built until that cell holds
  something**, the way the lyrics pane's `place` reveals nothing until the size it measured is
  the size it has: the `max(1)` fallback is a column count nobody wants drawn, and a frame of
  full-width cells stacked one to a row is the most visible thing the pane can do. The canvas is
  `absolute` and sized by the container rather than by the list, so it measures the same bounds
  whether the list is there or not, which is what makes holding it back safe. A resize still
  draws one frame at the count the last width gave, which is a cell or two out rather than a
  collapse. `Drawn::InAGrid` is the third image `Art` holds, at twice the cell
  like the other two.
- **An empty pane says what it is missing and what to do about it.** `kit::empty` draws the pane's
  own icon in a ring, one sentence naming the state and, where there is one, a second saying which
  gesture fills it, so "No albums yet" is followed by where to add a folder and an empty queue by
  where a row comes from.
- **A search that matched nothing offers the spelling the catalog holds, and the offer is a
  press.** `kit::empty_offering` is `kit::empty` with one more child, which is what the three
  browse panes' `RootView::nothing_matched` hands it: a `Tone::Outlined` button reading *Did you
  mean Billie Eilish?* that puts that text in the search field through `RootView::search_instead`,
  where the field's own observer takes it from there — so the correction goes in the box a listener
  can then edit rather than being run behind their back. What it draws is
  `LibraryModel::instead`, which `browsed` fills only where the albums, the artists and the tracks
  all came back empty, so the offer cannot contradict a pane that is listing something;
  `library.md` has how the spelling is found. All three panes ask through one method because all
  three empty on the same search, and the albums and artists panes build it before the heading, the
  borrow of the model having to end before `cx.listener` takes one.

## Drawing

- **An icon is an embedded SVG reached for by an `Icon` variant, never a text glyph, and one list
  is the whole of what an icon is.** The `icons!` macro takes a variant and the name it is drawn
  from, and writes the enum, `Icon::ALL`, `Icon::path` and `Icon::drawing` out of that one list, so
  a variant cannot be added to the enum and left out of the array the `AssetSource` walks — which
  used to compile, answer `Ok(None)` and render as nothing that no test could catch, because `path`
  and `drawing` were exhaustive and the test iterated `ALL`. `crates/resonate-ui/assets/icons` holds
  one silhouette per variant, `include_bytes!` is what embeds it, and `icons::Embedded` is the
  `AssetSource` the `Application` is built with, so a path nothing answers to cannot be written and
  the error enum needs no missing-asset variant. `Icon::Resonate` is the application's own mark and
  is the one icon drawn twice: the header's wordmark fills the accent square with it, and
  `packaging/resonate.svg` carries the same five paths under a `translate`-and-`scale`, over the
  rounded ground and in the gradient a launcher wants and an alpha mask cannot hold. Two files
  because a launcher icon is colour and a UI icon is a mask, and `icons.rs`'s own test is what
  keeps them one mark — it reads the `d` of every path out of both and refuses a difference, and
  reads `Icon=` out of the desktop entry to hold the asset's name to it, as `app.rs` holds it to
  `APP_ID`. gpui
  renders an SVG to an alpha mask and tints it with the element's `text_color`, so an icon carries
  no colour of its own and inherits nothing: every one is given a colour at the call site, and
  `icons::lit_on_hover` is what makes one follow its button's hover through a group. Every one is
  therefore a silhouette drawn to a 24-unit box, and a two-tone mark — a state badge over an icon —
  would have to be two overlaid `svg` elements. Text glyphs survive only in the window controls; the
  search field's clear mark, a row's ✕ and the queue row's arrows are `Icon::Close`, `ChevronUp` and
  `ChevronDown` now, and a control drawn to a box is what keeps its mark centred where a glyph's
  baseline did not.
- **The launcher's icon wears the accent, and the binary is what puts it there.**
  `AppIcon::of` is the whole decision and lives beside the palettes: an appearance whose worn accent
  is the colour `Appearance::DEFAULT` wears answers `Packaged`, and any other answers `Recoloured`
  with `packaging/resonate.svg`'s two gradient stops replaced by the accent lifted towards white and
  the accent itself, and its root given `id="resonate-in-the-accent"` — so switching palette moves
  the icon with the accent until one is chosen by hand, and going back to Resonate's own takes it
  away. `AppIcon::drawn_here` reads that id, and it is what makes the file ours to replace or take
  away: an icon under the same name nobody here drew is left exactly as it was. `run` shows the
  appearance it opens in and `RootView::dress` every one it wears after, through `Launcher`, a seam
  on `Stored` the way `Present` is, so `resonate-ui` does no I/O for it. The binary's
  `launcher::Icons` is behind it: a thread that waits `SETTLES_AFTER` of quiet so a run of presses
  is placed once, writes a staged copy over `$XDG_DATA_HOME/icons/hicolor/scalable/apps/resonate.svg`
  or removes it, and only where that moved anything flushes what the install scriptlet flushes —
  the theme folder's mtime, `kbuildsycoca6` and KIconLoader's `iconChanged`, which
  `resonate_mpris::tell_the_icons_changed` emits. A run opening in the appearance the file already
  shows writes and flushes nothing. **The gradient is in `userSpaceOnUse`**, spanning the mark's own
  24-box: under the default `objectBoundingBox` each stroke is a zero-width box, which the SVG
  specification says paints nothing, and librsvg drew the packaged icon as an empty tile.
- **The catalog is read while gpui starts, so the first frame already holds it.** `run` starts
  `FirstRead` before `Application::new`: a thread of its own reads what a model with nothing
  chosen asks for — `Asked::at_first`, which `LibraryModel::new` is also built from, so the two
  cannot drift apart — while gpui spends forty-odd milliseconds of the main thread loading every
  system font and bringing Vulkan up. `LibraryModel::new` takes it out of `ResonateApp`, and the
  first read takes it synchronously where it has landed, awaits it where it has not, and loads
  afresh where what the model now asks is not what was read ahead. Measured on a 616-track
  catalog, the first frame with the albums in it finished 105 ms after `main` where it had
  finished at 112 to 122, and no frame is drawn empty first; the rest of that start is gpui's own
  font scan and device creation, which nothing here reaches.
- **gpui draws cover art and `resonate-codec` is what scales one, so `resonate-ui` still carries no
  image crate.** `gpui::Image::from_bytes` takes the format and the bytes the library already
  stored, which is why `resonate-library` records the format on scan rather than leaving a decoder
  to sniff it. A picture out of the tag catalog arrives the same way, through `CoverArt` re-exported
  by `resonate-engine`, so `resonate-ui` takes no dependency on `resonate-codec` — it reaches
  `Drawing` through the same two re-exports. Each is read once and held in a `Recent` keyed by the
  album or the file, because a fresh `Arc<Image>` every frame would defeat gpui's own cache. The
  read and the decode run on `Drawer`'s two threads rather than on the background executor: a miss
  puts the key in the model's `decoding` set and hands the work over, the cell draws its placeholder
  disc until the picture lands,
  and the landing notifies the model, so a screen of unread covers costs the render thread nothing
  and a picture a file does not carry is cached as a miss rather than asked for on every frame.
  **A file's picture is cached only once the engine has settled it.** `Player::art` answers nothing
  on the ask that queues the read, so `PlayerModel::art_of` reads `Player::art_read` — `NotYet`,
  `Answered` or `Nothing`, the shape `TagsRead` has — and a `NotYet` is kept in `unsettled` against
  the `media_revision` it was asked at rather than in `pictures`, and asked again once the revision
  moves. Caching that first `None` as a miss is what left the playback bar's cover empty for any
  file the catalog does not cover, even one carrying its own picture.
  `LibraryModel::warm_the_covers` fills the album cache ahead of the grid once a run:
  `COVERS_WARMED` albums that declare a picture, decoded one at a time in a task of its own, so a
  scroll into a library already has its covers rather than decoding them as the cells arrive. One at
  a time is the whole of how it stays out of the way — it never has more than one decode queued,
  so a cell actually on screen waits behind at most one warming cover —
  and it skips what the cache or the in-flight set already holds. It is bounded under `COVERS_HELD`
  on purpose: warming more than the cache keeps would evict its own work.
  `DECODES_AT_ONCE` bounds how many are in flight, in the album cache and the per-file one alike: a
  cell that asks while the bound is full is refused *without* being marked as decoding, so it asks
  again on the next frame rather than being lost, and a landing notifies, which is what guarantees
  that next frame. Without it a fling queued one decode per newly visible cell onto an unbounded
  FIFO — every core busy on covers already scrolled past, ahead of the ones now on screen.
- **A cover is decoded on one of two threads of its own, because glibc gives every thread that
  allocates its own arena.** A 1280-pixel progressive JPEG costs about 15 MB while it decodes, and
  a block that size stops being mmap'd once glibc's dynamic threshold has risen, so it is kept by
  the arena of whichever thread freed it. Spread over gpui's sixteen workers that was some 110 MB
  held after the start-up warm — a window at 265 MB where the same window decoding on `Drawer`'s
  `DRAWING_THREADS` settles at 158, its peak falling from 280 to 158 with it. Two threads keep up
  with what the four-wide executor did, a cover now costing about 14 ms rather than 27. `Drawer`
  is a gpui `Global`: `draw` queues a closure and answers a future the model awaits, and a queue
  nobody reads — the threads could not be started — runs the closure where it was asked for rather
  than dropping it. The allocator was weighed as well and lost: mimalloc (v3 and v2) and jemalloc
  each settled higher than glibc on this workload — 575, 470 and 345 MB — because a worker that
  decodes and then idles never gives back what its thread cache holds.
- **What a cache lets go of, gpui is told to let go of too.** gpui decodes every `Arc<Image>` an
  `img` is handed into its own asset cache, keyed by the image's bytes, and nothing in it ever
  evicts: a cover the album cache had long dropped stayed decoded for the rest of the run, at three
  sizes, beside every magnified picture and every painted spectrogram. `Recent::insert` hands back
  what it pushed out or wrote over as a `Leaving`, and `Forget` turns that into `Image::remove_asset`
  — for the album covers, the portraits, the per-file pictures, the magnified picture a new one
  replaces and the spectrogram a new painting replaces — so what is decoded is bounded by what the
  caches hold. A picture still on screen that is forgotten is only decoded again. The sprite
  atlas's tile for it is not freed, because `drop_image` wants the `RenderImage`, which gpui hands
  out only through a `Window` and only by starting a decode of whatever size was never drawn;
  `docs/TODO.md` has it.
- **A cover is drawn at twice the size it is shown, because the GPU's sampler is all that stands
  between the texture and the cell.** gpui's atlas sampler is bilinear with no mipmaps, so it reads
  four texels however far it is reducing: a 1280-pixel cover in a 22-pixel cell was point-sampled
  and crawled. `Art` holds one image per cell a cover is drawn in — `Drawn::InARow` at twice
  `ROW_COVER`, `Drawn::NowPlaying` at twice `NOW_PLAYING_COVER` and `Drawn::InAGrid` at twice
  `GRID_COVER` — and `Drawing` decodes the cover once to write all three. Twice the cell is the
  size that matters: at a scale factor of 1 the sampler's four texels are exactly the 2x2 it should
  average, and at 2 the texture is drawn 1:1. Measured against a true area average over this
  library's covers, that is about sixteen times closer than handing the sampler the whole cover,
  where a 128-pixel thumbnail was only a third closer. The scaling is `CatmullRom`, run in linear
  light rather than on the sRGB values, and a cover already
  smaller than the cell is left alone rather than grown — the one case `Art` falls back to the
  picture as it came, built once under a `OnceCell` and shared by whichever of the three wanted it,
  so a cover large enough to scale copies its source bytes not at all.
- **The resize is `image::imageops::resize`'s arithmetic walked in the order memory is laid
  out.** The crate's vertical pass fixes a column and walks down it, a strided read per tap per
  pixel, and converted every tap to linear light as it went; that was two thirds of a cover's
  decode. `artwork::drawn_smaller` computes the same weights with the same expressions and sums
  each output pixel's taps in the same order, so the result is the crate's to the bit —
  `a_cover_is_drawn_exactly_as_the_image_crate_would_draw_it` holds it to `resize` over random
  RGBA — but it adds a whole source row into a column of sums at once, converts each source row to
  linear light once into `Held`, a ring as deep as the widest tap window, and runs the horizontal
  pass straight out of that column, so no intermediate image is built at all. A JPEG stays the
  `RgbImage` it decoded to rather than being copied into RGBA, and `squared`'s crop is a `Plane`
  over the same samples. The transfer function is a 256-entry table rather than a `powf` per
  channel per pixel, exact rather than approximate because the input is an 8-bit channel; what the
  8-bit hold costs is the intermediate precision of a 16-bit PNG, which is quantised on the way in
  and cannot show in a thumbnail written back out as 8-bit.
- **What the magnifier shows is read when it opens, not kept beside every thumbnail.** `Art` held
  the source bytes over again so a `whole()` could answer at once — a copy of the original picture
  per cached entry, across 512 covers, 256 portraits and 256 file pictures, for a view that shows
  one at a time. `LibraryModel::whole_cover` and `PlayerModel::whole_art` read it on the way in
  instead and keep one `Magnifying`: the key being read, and then that key with what it read, so a
  key that answered nothing is not asked again every frame, `magnified_art` being read once a
  frame for as long as the magnifier is open. It draws its scrim and its title and no picture
  until the read lands, and nothing else in the window pays for it. Nothing magnifies a portrait,
  which is why `Portrait` never had a reader for the whole picture at all. The two
  together take a window on a 435-track library of 1280-pixel covers from 689 MiB to 264.
- **A portrait is a picture cut square, and it is held apart from the covers.** `Portrait::of`
  draws through `Drawing::squared` where a cover draws through `no_larger_than`: the centred
  square of the shorter side, scaled down to the side asked for and never grown, so what the
  sampler is handed is already the square the round frame shows. It holds two images rather than
  `Art`'s three, because a portrait is drawn in a row and in the scoped heading's grid-sized frame
  and never as the playing track: `LibraryModel::portrait` takes a `Portrayed` — `InARow` or
  `InAGrid` — so a now-playing size is neither asked for nor paid for, and both share `Sizing`,
  the one decode and the one fallback to the picture as it came. `LibraryModel::portrait` reads
  out of `portraits`, a `Recent<ArtistId, Option<Portrait>>` under `PORTRAITS_HELD` of 256 beside the
  512 covers, decoded on the background executor under the same `DECODES_AT_ONCE` bound with
  `decoding_portraits` as its in-flight set, and only where `Artist::has_portrait` says there are
  bytes to read. `browser::portrait_frame` is the round frame — `ObjectFit::Cover` inside a
  `rounded` div — at `theme::avatar()` in the artists list and `theme::scope_cover()` in the scoped
  heading, and `kit::avatar` is what draws where there is none.
- **The magnifier's scrim occludes, so the press that closes it closes it and nothing else.** It
  is an `absolute` child painted over the whole app, and without `occlude` its hitbox was one more
  hitbox rather than a wall: the press that shrank the cover also landed on whatever row, cell or
  control stood under the pointer, so closing a cover over the albums grid opened the album
  beneath it. The menu's scrim already did this; the magnifier now does the same.
- **A cover is drawn wherever a row names a track, out of one cell.** `RootView::cover` takes a
  `Pictured` — an album, or a track that is an `Option<AlbumId>` beside the file it came from — so
  the albums, tracks, queue and playlists panes draw the same 22-unit cell. The album's art is
  preferred and `Player::art` is the fallback, which is what puts a picture on a row no scan has
  seen; a file carrying none loses nothing but the picture. `RootView::drawn_cover` answers with the
  art and a `Magnified` naming where it came from, so the transport's cover — the one that is
  clickable — opens whichever of the two it drew. The overlay that paints over the whole app is
  sized from `Window::viewport_size` rather than a constant, so it is as large as the window allows
  and no larger — the shorter window edge, capped, so it never fills a wide screen, and nothing
  pages from one cover to the next.
- **The window holds decoded pictures apart from the bytes they came from.** `Drawing` keeps 256
  tag-catalog pictures decoded beside 512 album covers, in `Recent`s bounded by count where the
  engine's `ROWS_HELD` and `ART_BYTES_HELD` bound the bytes, so neither window cache is bounded by
  what a picture weighs. A `Recent` keeps a `BTreeMap` from its use clock to the key beside the
  entries, re-keyed whenever one is looked at, so evicting is `pop_first` rather than a scan for the
  minimum over every entry held — the same shape the engine's `Held` takes, and what keeps a scroll
  past the 4 096-row bound from walking four thousand entries per newly visible row. A `Recent`
  evicts the entry nothing has looked at rather than emptying itself, so a row scrolled well past either bound is read again when it comes back — at up to two
  SQLite reads the first time a queue row is seen, one by id and one by path. Nothing empties
  `LibraryModel::named` on a library read, so what a counted play changes about a track has to be
  put back by hand: `track_heard` writes the row `Library::track_played` answered with into the
  cache, which is what keeps a queue row's count moving when the row is not in any listing the
  reload re-read.
- **Text that answers the pointer is lit when the element is built, because gpui cannot light it
  later.** gpui 0.2.2 resolves a `hover` style only in *paint* —
  `compute_style_internal(None, …)` in `request_layout` and `prepaint` — while a text child is
  shaped in `request_layout` with the colour of the moment baked into its runs. So a `hover` that
  changes a background works and one that changes `text_color` or adds an underline never
  reaches a glyph: every link `opens` drew, the column headers, a ghost button's label and a
  segment's all promised a hover that nothing ever painted. `views/pointed.rs` is the answer. An
  element that follows the pointer carries an `on_hover` that writes its `ElementId` into
  `POINTED_AT`, a thread-local set, and asks the window to redraw; the next build reads
  `is_pointed_at` and shapes the text in the lit colour, so the text is lit exactly as a selected
  row's already is. `LitUnderThePointer::lit_under_the_pointer` is the whole of the gesture — an
  id and what to do to the element while it is pointed at — and `follows_the_pointer` is its
  half for an element that lights something inside it, which is how an album cell lights its
  title. It is a set rather than one slot because a cell holds a link: the pointer over the
  artist under a cover is over both. `set_pane` and `show_everything` empty it, because a link
  pressed to go somewhere is taken away while it is still pointed at and never hears the pointer
  leave, and the pointer leaving the window empties it through `pointer_watch`. Where a control
  has a background it keeps its `hover` for that, and only the text goes through `pointed`.
- **A control names itself through one seam, and the seam is what lets go.** `hint::Names` is the
  only place a tooltip is attached and a test walks the crate's own sources to keep it that way. It
  is gpui's plain `tooltip` and never `hoverable_tooltip`, because
  `clear_active_tooltip_if_not_hoverable` is a no-op for the hoverable one: neither a press nor a
  scroll takes it down, and every hint here sits on something that is pressed or lives in a list that
  is scrolled. `hint::asking` is the other half — one atomic, written once a frame by
  `RootView::render` — and where it says no, no builder is attached at all, which is what gpui reads
  as an instruction to drop a live tooltip on its next prepaint. It says no while a slider is grabbed,
  while the picker or a magnified cover is up, while a row is being dragged, and while the pointer is
  outside the window. That last one is the reported defect: gpui's `MouseExited` arm is the one that
  does not update `Window::mouse_position`, so a pointer that leaves the window leaves the hint
  believing it is still hovered, and the 16 ms poll draws it there for the rest of the run.
  `RootView::pointer_watch` is a zero-size canvas registering `MouseExitEvent` and `MouseMoveEvent`
  window listeners from its *paint* — the idiom `field.rs`'s `follow_the_pointer` uses, and the only
  phase `Window::on_mouse_event` may be called in — and the flag it sets also closes the lyrics
  pane's hover, so a pane opened out by the pointer does not stay open once the pointer has gone.
- **Every control names itself in a tooltip, and a tooltip wants a pointer.** The playback bar, the
  sidebar rows, the window controls, the queue row's arrows and cross, every action in the tracks
  and playlists headings, each row's queue marks, the search field's clear mark, the info mark
  holding the search grammar and a folder's forget mark all carry one, and a toggle names the state
  it is in and the key that changes it. A list row says what it is only through the text it draws,
  so a session driven from the keyboard sees none of it.
- **A playlist row that no scan has seen draws itself the way a queued one does.**
  `views/listing.rs` owns the `Row` the queue and playlists panes both build: the scanned `Track`
  where the library has one, `Player::media` where it does not, and the file stem only where the
  file cannot be read at all. That is what lets an imported playlist of unscanned files read as
  titles and artists rather than as stems, and it costs `resonate-ui` no new dependency — the tag
  catalog is already what the queue pane draws from. The play count rides on the same `Row`, so a
  queue row and a playlist row draw it where a track row does, out of `listing::heard` — one cell
  `theme::row_plays()` wide that the tracks pane, both list panes and the playlists index all reach
  for, so no two of them can disagree about its width or its words. A row no scan has seen carries a
  count of nothing and so draws an empty cell, which is what the catalog already says about a file
  it has never met.
- **The cell says how often and how lately, because the catalog is ordered by both.** `played:`
  narrows and `SortOrder::Played` orders, and neither had anything on screen to be read against:
  when a track was last heard was drawn for the playing track alone, in the inspector's *Heard*
  card. `how_often_and_how_lately` folds the date into the count's own cell rather than taking an
  eighth column from rows that already carry seven — *12 plays · 3d* — and the header reads HEARD
  rather than PLAYS, which is the word the inspector's card, `listing::heard` and `COUNTS_AS_HEARD`
  already use for the pair. What makes it fit is `format::age`, the same `SPANS` table
  `format::since` reads written in its short spelling: one `Span` carries the singular, the plural
  and the brief, so a span cannot be named one way and forgotten the other, which
  `every_span_is_named_both_ways_and_no_two_read_alike` is the claim of. A row nothing has played
  draws the empty cell it always did, and a count with no date beside it — which is what a
  playlist's own reading can be — draws the count alone.
- **A catalog listing marks the playing track by the row it resolves to, never by the queue's
  id.** The tracks pane, Favourites and an opened suggestion compare against
  `RootView::playing_now`'s `track`, the library row `LibraryModel::track_of` weighs out of the
  queue row's location and span. They used to compare the queue row's own `TrackId`, which is the
  library's id only for a row loaded from the catalog in this run: a resumed queue is minted afresh
  by `Queue::restore`, so after a restart pressing Play lit nothing.
- **The row playing is the row a list is on, never a track id.** A file can be in a queue or a
  playlist twice, and a row no scan has seen has no `TrackId` to be asked about, so both panes ask
  which row the transport is on instead. The queue draws `PlayerState::queue_position`, the row of
  the order the engine publishes; a playlist draws `PlayerState::loaded_position`, the row of the
  order the queue was loaded in, which is the playlist's own order however the shuffle has moved the
  play order away from it. A playlist marks a row only while `Library::playing_playlist` names it,
  which it does only while the queue still holds the rows it was loaded with: the mark is sound
  exactly as long as the queue *is* that playlist, and a row added or dropped — by the window or
  over the bus — is what ends that.
- **A narrower read never cancels a wider one.** A read replaces `_load` and so drops the one in
  flight, which is right between two reads of the whole but lost the browse panes where a
  playlist edit's `ThePlaylists` read landed on top of a scan's closing `Everything`: the listing
  kept the rows the scan had just pruned. `reading_everything` is what the model remembers, and a
  read asked for while it is set reads everything.
- **A read of the library says how much of it it wants.** `LibraryModel::read` takes a `Wanted`:
  `Everything` reads the albums, artists, tracks and roots beside the playlists, `ThePlaylists`
  reads the listing, the opened playlist and its entries alone and leaves the browse panes where
  they stand. Every one of the seventeen gestures behind `LibraryModel::edit` writes to
  `playlists`, `playlist_entries` or `playlist_queries` and nothing else, so an edit, a change of
  listing order and opening a playlist all take the narrower one; a search, a selection, a scan, a
  counted play and forgetting a root take the whole, because each of those does move a browse pane.
  `Loaded::browsed` is `None` where the browse panes were not read, so `take` knows the difference
  between what came back empty and what was never asked for.
- **A read the typing asked for waits for the typing to stop; every other read runs at once.**
  `LibraryModel::set_query` is the one caller of `read_after`, and `SEARCH_SETTLE` is the 150 ms it
  holds the read for, so ten characters cost one pass over the albums, the artists, the tracks, the
  roots, the playlist index with its `count(*)` per saved query and the opened playlist's entries
  rather than ten. What the keystroke does move is `Search::read` and the query beside it — a
  handful of tokens — so the *Reads* row and a pane's own narrowed reading are live as the box is
  typed and only the SQLite work settles. The wait lives in the `_load` task, so a keystroke inside
  it drops the one before rather than queueing a second.
- **A library notice says whether it went well, and in what voice.** `LibraryModel::notice` is a
  `Notice`, which is `Trouble`, `Done` or `Noted`: import, export and tidy achieved what was asked
  and go out in the palette's green, a finished enrichment merely happened and goes out in
  `theme::muted()`, and a red line is the wrong way to say either. `looked_up` says *34 albums and
  12 artists answered* and leaves the eight-field breakdown in the Online card, which is where
  someone goes to read it; the sidebar's enrichment line wears `faint` and `muted` rather than the
  accent, so a run in progress is not the brightest thing on screen. `listing::noticed` picks the colour, so the settings pane, the
  tracks heading and both playlist headings cannot disagree. The tracks heading draws one because a
  queue gesture pressed there has nothing else to report through: the playback bar's count moves,
  but only the notice says where the rows landed.
- **The device name is not in the playback bar.** The sink's description was the one child of the
  signal path with no width of its own, so a long one pushed the panel to its clip edge and took
  the album title with it. The inspector's OUTPUT stage names the device and the settings row
  keeps its node name behind the pointer; the signal path's hint says *Playing through …* before
  what it always said, which is the whole of where it went.
- **The position is polled every 16 ms and drawn only where the drawing would change.** The
  window is one `RootView`, so a notify from `PlayerModel` lays out and paints the whole tree, and
  a position that moved every poll made a playing window cost 10 to 11 % of a core where
  `resonate play` costs under one. `Grain` is what the render tells the model the moment is drawn
  at: on the lyrics, the visualiser and the inspector every poll, because the line, the spectrum and
  the latency follow it; everywhere else a quarter of a device pixel of the seek rail —
  `STEPS_PER_PIXEL` over `Rail::width` times the scale factor — and never coarser than the clock's
  second, so the elapsed time turns over when it should and `Listening` and `Keeping` never see a
  step anywhere near the `A_SEEK` they read as a seek. `Grain::shows` lets anything but the
  position and the sink's latency through at once — a pause, a seek, a track change, the sleep
  timer's second — and a rail not yet painted, or a thumb held, is every poll. The model's state
  is still refreshed every poll, so whatever else asks for a frame draws the moment as it is; what
  it saves is the frames that would have painted the same pixels, taking a playing window on the
  tracks pane to 2.7 %.
- **The playback bar is three columns and the transport is the middle one.** The now-playing panel
  and the status cluster are both `flex_1` with a basis of zero, so they take equal halves of what
  the centre leaves and the buttons sit on the window's centre line whatever is beside them. The
  centre is a column with a basis of `theme::transport_centre()` that shrinks no further than 300,
  holding the step and play buttons over the seek rail with the elapsed and total clocks at its
  ends; the rail is the one that fills its row, which is what `Handle::fills_its_row` names. Both
  sides carry `overflow_hidden`, because a badge or a notice wider than its half would otherwise
  paint over the controls rather than be clipped. The now-playing panel is the cover at
  `theme::now_playing_cover()`, the title, the artist and album, and under them the signal path: the
  format badge of what is decoding with its depth and rate, then the output's mode dot and its
  name in the mode's colour — *bit-perfect*, *converted* — and nothing after it. It carried a link
  mark and the negotiated depth and rate as well, which doubled the line's length to say what the
  mode's name and the inspector's *Output* stage already say; what a converted stream became is
  the inspector's to spell out. It is the same line the inspector's three stages expand on, and a
  press on it is what opens that pane, because the short reading and the long one should not be
  two unrelated places. The title, the artist and the album are three
  elements rather than one truncated line: each opens the page behind it through `RootView::opens`,
  the title and the album scoping the tracks pane to `Selection::Album` and the artist to
  `Selection::Artist`. Which of them is pressable is what is *known* rather than what is written:
  the album id rides on the `Cover` the panel already resolved and the artist id on
  `Track::artist_id`, so a row no scan has seen draws all three as plain text, the names having
  come off the file's tags with nothing behind them. `RootView::by_line` is the artist, the
  separator and the album as one method, and it takes the two element ids it is to draw under, so
  the inspector's heading is the same line rather than a second copy of it. The by-line reads as the one line it used to
  be because `keeps_its_width` holds the artist at `flex_none` under a `max_w_full`: three
  truncating children shrink in proportion to their own length by default, so an album title four
  times the artist's took a quarter of the loss out of the artist's name — the album is the one
  that gives way, and the artist ellipsises only where it alone overruns the panel. **What gives
  way ends in an ellipsis rather than a square cut, and each is given a width it can be cut at.**
  gpui's text element truncates only inside its measure, and it keeps the first measure of a
  no-wrap text whatever width it is later handed, so a `truncate`d name inside a content-sized
  flex item is laid out whole and sliced by its parent's clip; a `line_clamp` on the same item is
  measured at its min-content width and drew *The Tr…*, or nothing. The album is `flex_1` under
  `ends_in_an_ellipsis`, so it is measured at the room the row leaves it. The title cannot be
  `flex_1`, because the star rides right after it, so `kit::cut_to_fit` cuts the text itself —
  gpui's own `LineWrapper::truncate_line` at the width `RootView::playing_room` measured a frame
  behind, less the star — and the element draws the shortened string at its own width. The
  inspector's stage cards are `flex_1` already and take the clamp. A notice the engine raised is not in either half: it is a strip of its own above
  the bar, the full width of it over a wash of the failure colour, truncated with the whole of it on
  hover and taken away by a click anywhere on it or by escape away from the search field, because a
  load that was refused starts no track and `TrackStarted` is what otherwise clears one. What it
  says names the command rather than debug-printing it: `CommandKind::as_str` is the prose, so a
  refused `Load` reads *the load was refused* and not the whole of the queue items it carried. Every
  control in the bar names itself through `views/hint.rs`, because a
  silhouette does not say what it does: a step names its key, and a toggle names the state it is in
  and what the key does next, so the accent is not the only thing reporting it. A rail is drawn
  filled in `TEXT` and turns to the accent under the pointer or while held, and its thumb is drawn
  only then. The volume's reading is always drawn, in `theme::volume_reading()` of room and in the
  body face: it used to be at `opacity(0)` until the pointer was over the cluster, which reserved
  the room without ever filling it and read as a hole between the rail and the window's edge.
  `kit::readout` is what draws it — `theme::ui` is the body face with `tnum` on, so the figure is
  as steady under the pointer as `kit::figure` is without being mono, which a percentage beside a
  slider has no reason to be.
- **The play mark carries its optical nudge in the art, not in a margin.** A right-pointing
  triangle centred by its bounding box reads a little left, because its mass is on the base, so
  `play.svg` is drawn half a unit right of the 24-box's centre and the button adds nothing. What it
  replaces is a `margin-left` of 2 px on the play state alone: that put the triangle some two and a
  half pixels right of the disc's centre and, because `pause.svg` is drawn dead centre and took no
  margin, moved the mark sideways every time the transport was toggled. Anything an icon needs to
  sit right belongs in the icon.
- **A lyric look starts when the track does, not when the pane is opened.** `look_for_lyrics` runs
  on `RootView`'s observer of `PlayerModel` — the one `count_a_play`, `Keeping` and `Following`
  already ride — so a track change searches whatever pane is in front and the pane opens on a set
  rather than on *Looking for lyrics…*. It costs nothing idle: `Asked` is `Copy`,
  `worth_looking_again` refuses a repeat, and the look already runs on the background executor.
- **The whole sheet opens out only where the pointer is on the words.** `near_the_words` is the
  decision — inside the centred column band, and within `lyric_reach` of the line being read —
  and it takes bounds and answers a `bool`, so it is tested without a window. The view reaches it
  through the `pointer_watch` idiom: a zero-size `canvas` registering a `MouseMoveEvent` window
  listener from *paint*, which is the one phase `Window::on_mouse_event` may be called in. An
  `on_hover` on the pane opened the sheet out from the empty gutters and from either far end.
- **The lyrics pane follows the playing track and nothing else.** `RootView::wanted_lyrics` builds a
  `Wanted` from the queue row's location and the `StreamDigest`'s tags, filling the title, the
  artist, the album and the length off the scanned library row until the digest lands — the album
  through `LibraryModel::album_title`, the length in the row's own rate — so the first look has
  everything a provider searches on and a `[length:]` to weigh a sidecar against. What a redraw compares is `lyrics::Asked` — the track id, its duration and whether a
  digest for that track has landed — which is `Copy` and says everything that could change what is
  wanted; `Wanted` itself is built only once `LyricsModel::asks_again` says the reading moved, so a
  60 Hz redraw no longer clones the title, the artist, the album and the whole embedded lyric text
  out of the digest in order to find out they are the same. `follow` resets the pane only where
  the *track* moved, so a digest landing mid-track refines the look without rewinding the scroll —
  and the lookup that refinement starts rewinds on landing only where the track moved or it
  answered a sheet other than the one on screen, `already_shows` being that weighing — and `worth_looking_again` is what decides whether there is a look to make: the registry is asked
  again on a track change, and otherwise only where the `Wanted` itself differs from the one last
  searched on. `Asked` moving is not enough, because the `tagged` flag flips exactly once per
  track when the digest lands — so every scanned track, whose row already gave the title and the
  artist the digest then repeats, used to pay a second search that could only answer what the
  first had. A track no scan has seen still pays two, and earns them: the first searches on the
  location alone, which is what finds a sidecar, and the second on what the file turned out to say
  it is. The look runs on the background executor, because a provider that
  reaches a network must not block the frame. The lines are turned into `SharedString`s once, when
  the look lands, so a 60 Hz redraw hands each line a reference count rather than copying the whole
  sheet out of the set every frame.
- **The lyrics heading says where a set came from, and what the set says about itself rides behind
  the pointer.** `attribution` is the readback the heading ends with: a `SYNCED` or `UNSYNCED`
  badge, the provider's own name as a `kit::figure`, and — only where the sheet credited anyone —
  one more figure naming whoever laid it down. Which name that is, is the window's choice and not
  the crate's: the `[by:]` transcriber, the `[au:]` author of the words where nobody signed the
  sheet, and the editor where neither is named, so the row draws the most particular claim the
  sheet made and nothing where it made none. The rest is the hint on that one figure, through
  `hint::Names` like every other: the words' author, the sheet's, the editor with its `[ve:]`
  version and the `[al:]` record, each sentence written only where the sheet wrote it. One figure
  is all the crowding the row will take — Follow, two reading chips, the press hint and the two
  attribution marks are already in it — which is why a sheet that declares everything reads as one
  name and hovers as four sentences.
- **The pane always draws and scrolls the whole sheet; a reading is only how far the light reaches.**
  `Falloff::Around` looks only forward — the line being sung and the two coming after it, with
  everything already sung out — which is what `Reading::InPlay` is; `Falloff::Across` steps down
  gently in both directions so `Reading::Whole` stays readable to the edges, and the type is bold
  and centred in its column. A growing line's size is stepped at `SIZE_STEPS_PER_PIXEL`, because
  rounding it to whole pixels gave the 420 ms turn eight steps and it read as a low frame rate.
  **A row is measured at the size it will reach, not at the size it is drawn at.** The line box is
  `text_lyric_lead()` times `LEADING` whatever the line's own size is, and the text is laid out in
  `inside * size / text_lyric_lead()` of room — the padded column scaled by how far the line has
  grown — so the ratio the wrap point is decided by never moves and a row takes the same number of
  rows, at the same height, lit or at rest. Without both, a line that sat on one row at rest took
  two as it was lit: its height changed mid-turn, every line below it shifted by a row, and
  `centre_of` read bounds that were still moving, so the sheet slid under itself rather than
  arriving. What it costs is the difference in leading on every line that never grows, which is
  whitespace rather than motion. A line is drawn for
  what is coming, not for what has been: keeping the last one up behind the sung one is what makes
  a pane read as a transcript rather than as a track playing, so `Falloff::behind` is `0.0` for the
  first and `ahead` mirrored for the second. Drawing every line either way is what makes the motion possible at all: an
  `InPlay` reading that built a fresh three-line column each time would have nothing to scroll, so
  the lines would swap in place instead of sliding. The choice is a chip in the heading and lives
  only as long as the run, like the settings pane's category, and the pointer over the pane opens
  it out — `LyricsModel::shows_every_line` is the one answer both go through. An unsynced set has no
  lit line, so every line stands at `ADRIFT` and no chip is offered.
- **Two voiced lines have separate reading edges.** A set with a second voice places voice one on
  the leading side and voice two on the trailing side, with a small voice label when the singer
  changes. Text and alignment both identify the voice; the second also takes the accent when lit.
  Each voice's active line brightens and grows on the existing turn, even where both sing together.
  A set with one voice keeps its centred column and its former width.
- **The falloff counts written lines, not rows.** `drawn` records each line's ordinal among the
  non-blank ones when the look lands, so a blank line between verses costs its neighbours no
  standing and the reading is the same three lines across a verse break as within one. Reaching for
  it per frame would be a walk per line per line.
- **Three `Turn`s on one `TURN`, and a `Glide` on a spring.** Where the pane *reads* and what the
  pane *lights* are not the same line, so each has a clock of its own. `Reads` is what the light
  turns on: `At` a line while one is in play, `Spent` where none is, `Evenly` for an unsynced set
  that never lights one. Ten seconds into an instrumental `line_in_play` gives out and the set goes
  to `Falloff::spent` — nothing at all in an `InPlay` reading, a faint 0.16 in a `Whole set` one,
  because a reading asked for deliberately should still be readable — while `read_at` keeps the
  sheet where it is. That is also what takes the last line of a set away once it has had its word,
  rather than leaving it lit for the whole outro. `standing` blends over `Reads`, `lead` over the
  lines in play, and the size and colour are both read off `lead` — `mixed` lerps `muted` to `text`
  — so a line grows and brightens over one 420 ms rather than snapping at a threshold. The third
  turn is `spread`, over the `Falloff` itself: the pointer opening the pane out and a reading chip
  both go through `Turn::onto`, so the lines a wider reading brings up fade in over the same span
  rather than appearing at once, and `standing` is the falloff turn blended over the reads turn.
  The `Glide` is different in kind: it carries the scroll offset to `centre_of` the read line on
  `spring`, a closed-form damped spring of `GLIDE_RESPONSE_SECS` and `GLIDE_DAMPING` that settles
  within `GLIDE_SETTLES_IN` with a two-percent overshoot, because a move that decelerates into its
  landing reads as the sheet arriving where an ease-in-out reads as it being pushed there.
- **A line further down the sheet sets off later, so a change ripples rather than shifts.** Every
  row is two boxes: the outer one is what `bounds_for_item` measures, and the inner one is
  `relative()` with a `top` inset of `LyricsModel::lag` plus `LyricsModel::rise`, so nothing the
  motion does moves the bounds the landing is computed from. `Glide::lag_of` is the whole of the
  ripple: a line `n` written lines past the read line reads the same spring `LAG_PER_LINE × n`
  later, capped at `LAGS_AT_MOST`, and its inset is the distance between where the sheet is and
  where that lagging clock says it should be — so the lines below the sung one are still catching
  up as it lands, which is what Apple Music's sheet does and what a single offset for the lot
  cannot. Lines above the read line lag nothing, because what has been sung is out of the way
  first. `Glide::settled` therefore waits the last lag out, so the frames keep coming until the
  furthest line is home.
- **A set arrives rather than appears.** `place` starts `arrived` the moment the pane is placed,
  and two readings come off it: `arrival` is the body's opacity, an ease over `ARRIVES_IN`, and
  `rise` is each line's inset, `RISES_FROM` down and springing to nothing `RISE_PER_LINE` later for
  each written line it is from the read line in either direction, capped at `RISES_AT_MOST`. Until
  the pane is placed `rise` answers the full `RISES_FROM` and `arrival` answers nothing, so the
  first frame lays out low and invisible and the set comes up out of the read line. `breath` is
  read off the same clock: a sine over `BREATH`, which is what the gap's dots swell on.
- **A look that has only just started is kept quiet.** `follow` stamps `asked_at` where the track
  moved on, and `looks_quietly` answers true for `LOOKS_QUIETLY_FOR` after it while the look is
  still `Searching`; the pane draws an empty body in that time rather than *Looking for lyrics…*,
  because a sidecar answers in a few milliseconds and a placeholder that flashes between one set
  and the next was the least seamless thing about a track change. It asks for its own frames, so a
  look that does take longer is announced when the grace runs out.
- **A gap is read at the line it is waiting for, not the one just sung.** `read_at` prefers
  `Waiting::next` and falls back to `line_at`, so an instrumental scrolls the upcoming line to the
  middle and lets it rise there under the dots while everything else is out — and past the last line,
  where there is nothing to wait for, it falls back and holds.
- **`centre_of` reads the laid-out bounds, and the glide bends rather than restarting.**
  `ScrollHandle::bounds_for_item` is what gpui laid out with no scroll offset in it, so the landing
  *is* the offset and adding the current one would make the target chase the glide and never settle.
  The target still drifts a little every frame while the lit line is growing under it, so
  `glide_to` moves `lands` in place while a glide is in flight and a drift is under `RESETTLE`, and
  starts a fresh one only for a jump. Restarting on every drift was what froze the scroll near its
  start and then let it snap.
- **A pane is drawn nowhere until it has been placed, and a row is as wide as the pane says.**
  The sheet's spacers are measured off `ScrollHandle::bounds`, which is a frame behind, so the first
  frame lays out with no padding at offset zero and the second lays out with the padding but centres
  off the first's child bounds. Two frames of a sheet sliding into position is what "it glitches and
  then corrects itself" was. `place` therefore reveals nothing until the size the layout was
  measured from is the size it has now, snaps the offset rather than gliding to it, and reports
  itself as moving throughout so the frames keep coming while paused; until then the body is drawn
  at `opacity(0)`, which lays it out without showing it. `follow_the_track` snaps its turns over the
  same span, so a set arrives at its standing rather than fading in from `Reads::Evenly`. `rewind`
  puts it all back, so a track change places the next set afresh instead of gliding in from where
  the last one sat. Each row takes an *absolute* width, `LyricsModel::column_width` — the pane's
  measured width less `theme::lyric_gutter()` each side, capped at `theme::lyric_column()` — rather than `w_full` under
  a `max_w`: taffy fixes a flex item's height from a measure taken before a percentage width has
  resolved and never measures again, so under `w_full` every wrapped line was laid out one row tall
  while gpui painted it wrapped, and the row after it drew over its second row. The head of a synced
  sheet is the container's own top *padding* and the tail is a spacer *child*, which is not a pair
  of half-measures: `content_size` is the extent of the children alone, so a leading child would put
  every line one row off what `bounds_for_item` answers and leave the sung line a line's height
  below the middle, while a trailing padding would leave nothing to scroll into and the last line
  would never reach it. The pane asks for its own frames — a zero-size `canvas` calling
  `Window::request_animation_frame` while any clock is in flight — because the 16 ms poll notifies
  only when the player's state *changed*, so a turn started by a press, a seek or a chip while paused
  would otherwise freeze half way.
- **A press on a line is a seek, and a scroll of your own is not fought.** Every timed line carries
  `RootView::seek_to_moment`, which clamps to the track's duration before sending `Command::Seek`,
  because the engine refuses a frame past the end and a hand-written `.lrc` whose last timestamp
  overruns the file would otherwise leave a red line in the playback bar that only a track change
  clears. A wheel over the pane marks it `led_by_hand` and the pane stops following for `HANDS_OFF`;
  the heading grows a *Follow* button while it is held off, and that press, a press on a line or a
  track change is what resumes it.
- **A gap breathes, and nothing else marks the moment.** `Lyrics::waiting_at` answers only where no
  line is in play: which line the gap is waiting on and how far through the wait the transport is,
  counting from the start of the track before the first line. The pane draws it as three dots
  standing where that line will be, filling in turn and swelling on `LyricsModel::breath` by
  `DOT_SWELL` over `LYRIC_DOT`, with the line itself rising towards lit as the count runs out. The
  dots sit in a row of the *fully swollen* height with the padding outside it, so the row holds
  still while they breathe: sized to the dots themselves, the gap grew and shrank and shifted every
  line under it by a pixel or two, which is the one thing left in the column whose height moved
  while it was drawn. It
  lives in `resonate-lyrics` beside `LIT_AT_MOST` rather than in the window, so the ten seconds is
  one constant and not two. Nothing draws progress *through* a line: the set has no word timing, so
  a rail under the lit line would be a line's whole span pretending to be a karaoke sweep, and the
  seek bar already says where the track is.
- **A name that names something the window can open is an `opens`, and the id it opens is what
  decides.** `RootView::opens` takes an `Option<Selection>`: `Some` draws the name as a link — the
  accent and an underline under the pointer, the cursor, the hint and a listener that selects and
  shows `Pane::Tracks` — and
  `None` draws the same truncating element with none of them, so one call site covers a scanned row
  and an unscanned one without a second branch at every name. It is the whole of how a name is
  pressed, and every one of them goes through it: the playback bar's title, artist and album, the
  inspector's and the lyrics pane's headings, the owner under a scoped album heading, the artist
  under an album's cover in the grid, and the artist cell of every track row in the tracks, queue
  and playlist listings. The listener stops the press, because a row is itself a press — clicking
  the artist in a track row opens the artist and does not also start the track, the same way the
  queue and playlist marks in `row_controls` stop theirs. The link is `keeps_its_width` inside a
  cell, so it is as wide as the name and no wider: the rest of the artist column belongs to the row.
  What a row knows is what `listing::Row` carries — `artist_id` beside the `artist`, filled by
  `listing::scanned` and left `None` by `read` and `unread` — so a queue row the library has never
  seen draws its artist as text. `unheld_row` passes `None` outright, because a release track the
  catalog holds no row for names an artist the library has no id for; it goes through `opens` all
  the same, so the cell is laid out by one function rather than two.
- **A name that runs past its room ends in an ellipsis, and `kit::EndsInAnEllipsis` is the one
  place that knows how.** A `truncate`d name — `overflow_hidden`, `whitespace_nowrap` and
  `text_ellipsis` — is cut where its room ends and nothing says it was: inside a cell of fixed
  width the last letter is sliced through, and under the album grid's caption, a content-sized
  flex item, the ellipsis never came at all, because taffy measures such an item under
  `MaxContent`, gpui's text cache answers that measure for every later measure of a `nowrap`
  text, and so the line is laid out at its whole width and the truncation never runs.
  `ends_in_an_ellipsis` is `truncate` with `whitespace_normal` and `line_clamp(1)` over it: a
  single-line *wrap* escapes the cache, because a wrapping measure is asked with the width it
  has, and the clamp is what draws the ellipsis at it — `text_ellipsis` is still what says which
  glyph, so the three go together and the trait is where they are written. `opens` takes it, so
  every link this window draws ends in one — the playback bar's title, artist and album, the
  inspector's and the lyrics pane's headings, the artist cell of every row, the grid's caption
  and the Missing pane's headings alike — and no call site adds it a second time.

## Panes

- **A favourite is a star, because the heart is taken, and what is missing is neither.**
  `Icon::Want`/`Wanted` are already an outline-and-filled heart and both appear on a track row, so
  a second heart would have been two marks nobody could tell apart. The heart is the *gesture* of
  wanting a row and nothing else: the Missing pane, its empty state and an artist's *not held*
  button are `Icon::Missing`, the disc with its rim dashed away, because a pane of absences drawn
  as a heart read as a second favourites list; `Favourite`/`Favourited` are a star on the same 24-unit box at
  the same stroke weight. The mark hides on hover like every other row control **unless the row
  is already a favourite**, which is the one state that has to be legible at rest — and
  everything that fades has to be inside the thing that fades: the tinted ground behind the mark
  on an album cell began as a wrapper the opacity never reached, so every cover in the grid wore
  a grey square until the ground was moved onto the mark itself. It is the general rule, not a
  one-off: a conditional control's ground, border and padding belong inside the condition.
  **A favourite is drawn filled and in the accent** — `kit::lit_mark` — and an unmarked one as the
  grey outline every other row control is, so the two states differ by more than a fill nobody
  could see in grey at fourteen pixels.
- **What was favoured is what the star says from the press on, not from the next read.** The
  star used to draw whatever the last read of the catalog said, and the playback bar's and the
  queue's rows read it out of `LibraryModel::named`, a cache nothing re-reads — so a press wrote
  the favourite and the star stayed empty, and the next press, still reading *not a favourite*,
  wrote it again rather than taking it away. `LibraryModel::favour` now notes what it wrote in
  `favoured`, a map from `Favoured` to the answer, puts the answer into the `named` row where the
  track is cached, and notifies before the write has run; `LibraryModel::favours` is what every
  star, every cell and every menu asks, with what its own row read as the fallback, and
  `favoured_album` and `favoured_artist` go through it too. The map lives as long as the run and
  is never wrong within it, because every favour this window makes goes through `favour`.
- **Three panes were added and each one is the same six edits.** `Pane::Favourites` and
  `Pane::Suggestions` sit under `Section::Collection` and `Pane::Statistics` under
  `Section::Library`; each is a variant, a place in `Pane::BROWSE`, arms in `label`, `about`,
  `section` and `icon`, an arm in `RootView::content`, a sidebar count and a `mod` line.
  Favourites stacks the three kinds — a shelf of artists, a shelf of albums and the ordinary
  track rows — rather than listing tracks alone, and its sidebar count is all three, because a
  pane holding only favourite albums would otherwise read as empty. Its order is fixed at
  *Favourited*, so its column header is an unsorted one: reusing the tracks pane's would have lit
  that pane's order and sorted the wrong list.
- **The statistics chart is `div`s, not a canvas.** `inspector::traced` and the equaliser curve
  are continuous series where this is a few dozen discrete bars, and a `div` carries a
  `hint::Names` hover for free where a canvas would need hit-testing written by hand. A day
  nothing was played on still draws its baseline, so the axis has no holes, and a run longer than
  `BARS_AT_MOST` folds whole days into one bar rather than drawing a year a pixel at a time. The
  axis reads relatively — *today*, *9 days ago* — because there is no date crate in the tree.
- **A suggestion card reads its rows off the frame.** `RootView::with_the_rows_of` is the sibling
  of `with_everything_listed`: a suggestion is a search and may name the whole library, so Play
  and Add to queue read it on the background executor before they act. *Save* is
  `Library::save_query` through `LibraryModel::edit`, so a name the catalog already holds comes
  back as a `Notice::Trouble` rather than as a panic, and a card whose search a playlist already
  fills itself from says *Saved* under `kit::Press::Greyed` rather than offering a second copy.
- **The Suggestions pane is shelves by kind, and a card opens onto what it would hold.**
  `SuggestionKind` groups the cards under *From your listening*, *Eras*, *Genres*, *Artists* and
  *Sound*, in `SuggestionKind::ALL`'s order, each an eyebrow over a wrapping row. Pressing a card's
  art or its name is `LibraryModel::open_suggestion`, which keeps the `SavedQuery` it opened in a
  `Previewed` and reads the first `PREVIEWED_AT_MOST` rows on the background executor; while the
  opened query is still among the offered suggestions the pane draws it instead of the shelves —
  a way back, the art at `scope_cover`, the kind, the name, the reason, the count and length, the
  search as *Reads* chips, and *Search for these*, *Save*, *Play next*, *Add to queue*, *Shuffle*
  and *Play* over an ordinary unsorted track listing, each row playing the list from itself. A
  suggestion the catalog stops offering takes the pane back to the shelves on its own. *Search
  for these* is `search_instead` and `choose_pane(Pane::Tracks)`, so the list can be narrowed
  further and saved under a name of its own. *Shuffle* loads the rows from a place picked off the
  clock's nanoseconds, there being no random crate in the tree, and then turns the transport's
  shuffle on.
- **A suggestion's art is drawn, never stored.** `suggestion_art` draws the covers the library
  answered in `Suggestion::pictured_by` — one whole, or two to four as a 2×2 mosaic whose empty
  tiles are the ground tinted — over a 135° gradient between two accents of the worn palette,
  read through `theme::hue` so the art follows the theme. The accents are the reason's where it
  has a colour of its own — a decade amber into peach, *Most played* red into peach — and a
  genre's or an artist's are picked by an FNV hash of the name, so one name is the same two
  colours every run and two names differ. With no cover at all the ground carries the reason's
  icon and the name in bold, in `theme::ink_over` the first accent. A cover is asked for through
  `LibraryModel::cover` at the grid's size like any album cell, so the cards fill in as the
  decodes land rather than holding the pane. **The gradient is painted only where no cover is.**
  It used to lie under the whole frame, and gpui clips a child to its parent's rectangle rather
  than its rounding, so the square covers stood over the rounded ground and a hairline of it showed
  along the frame's edge and wherever two half-pixel tiles met. The side is a whole pixel, a tile
  is the floor of half of it, each tile and its cover are rounded on the one corner they stand in —
  `Corner::of_tile` — and a single cover is rounded itself; an empty tile is its accent, solid.
- **The playlists index row grew its first context menu, and that cost its controls a guard.**
  gpui fires `on_click` for a right press, so once the row answers a menu every one of its five
  pre-existing controls — play, next, last, copy, discard — needs `menu::pressed(event)`, or the
  press that opens the menu also fires whichever control it landed on. A pinned row wears a
  `kit::badge` beside the `SEARCH` and `KEPT` ones; the catalog already sorts pinned rows first,
  so the pane does no ordering of its own.
- **The sleep control draws no clock of its own.** It sits between repeat and the volume in the
  playback bar and opens a `Menu` rather than cycling, because there are eight choices and a
  toggle would have to be pressed through them. The accent wash and the countdown are both inside
  one `when_some` on `PlayerState::sleeping`, so an unset timer leaves a moon at the same weight
  as shuffle and repeat and nothing else; the reading comes off the publication, and a part
  second rounds up so a timer just set reads `15:00` rather than `14:59`. It is deliberately not
  a setting and has no config key: a sleep timer is a decision per night, and a key with no
  control is what the settings rebuild went looking for.
- **The queue is reached from the playback bar, not the sidebar.** `Pane::BROWSE` is what the
  sidebar lists and `Pane::Settings` is pinned under it by a `justify_between`; `Pane::Queue` is
  reached only through the bar's queue button, a `toggle` like shuffle and repeat carrying the
  accent that says it is open. It carries no count: the queue's own heading says how long it is,
  and a figure beside the one icon in the cluster that had one read as a badge to be cleared. `RootView::behind_queue` remembers the pane the button covered, so a second press
  returns to it rather than to a default.
- **The queue follows the row that started playing, and only while it is the pane in front.**
  `RootView::following` is a `Following` — the row last scrolled to, and nothing else — and
  `Following::follows` is the whole of the decision: the row `PlayerState::queue_position` names, or
  nothing where the row has not moved since the last scroll, where `self.pane` is not `Pane::Queue`,
  or where nothing is playing. It rides the `cx.observe(&player, …)` that `count_a_play` already
  rides, and it scrolls through `show_row`, the same `ScrollStrategy::Center` move the reach keys
  make, rather than a second call to `scroll_to_item`. Holding the row is what keeps the 200 ms poll
  from fighting the listener: a redraw that leaves the playing row where it was scrolls nothing, so
  a queue scrolled away from by hand stays where it was put until the track changes. A row that
  moved while another pane was in front is not recorded as shown, so the first poll after the queue
  comes back lands on it — the row the list was left on is no longer the one playing, and scrolling
  a list nobody is looking at only yanks it later. `Following` is arithmetic over an `Option<usize>`
  and nothing else, so `views/root.rs` tests it without a window the way `views/reorder.rs` tests
  `Step::landing`.
- **The *Reads* row is a readback of the search box rather than a claim about a listing.**
  `listing::reads` is one chip per clause off `Search::reads`, and it stands in every pane's
  heading — under the tracks pane's, the playlists index's and an opened playlist's, and beside the
  albums and artists panes' titles, which carry their counts and nothing else. It says how the text
  was read, not what each pane did with it: `Matching::grouped` carries the whole
  grammar into the albums, artists and tracks listings and `paths_matching` into an opened
  playlist's rows, while the playlists index narrows on `words_of` alone, because a playlist has
  only a name to answer with — which is what its empty message says. A lone word draws no chip
  anywhere, because a word needs no explaining. A pane with nothing left to list says "No albums
  match." where a search is in force and "No albums yet." over where to add a folder where none is,
  so the branch that blames a search is the branch a search caused.
- **The window keeps the queue for the next run, and the switch that stops it discards what was
  kept.** `Keeping` sits on `RootView` beside `Listening` and runs off the same observer of
  `PlayerModel`, so the window keeps what `resonate play` keeps and by the same arithmetic — see
  `audio.md`. `Stored::resume` is what the binary hands in, because the flag is a `config.toml` key
  the window has to draw as well as obey, and `LibraryModel` holds it beside `online` and
  `after_scan` for the same reason. The Resuming card is Library's third group, not Online's:
  what it turns on writes to the catalog and reaches no network. Turning it off is
  `Library::forget_resumption` at once rather than merely ceasing to write, because a queue left
  behind by a setting that is now off would come back the next time it was turned on.
- **The window is handed its lookups and owns none of them.** `Lookups` is what `run` takes
  beside the player, the library and the settings: the `Lyricists`, an `Option<Arc<dyn
  Reference>>` and the `Online` setting — `enabled` and the `contact` as saved — and `ResonateApp`
  holds all three as globals, so `LibraryModel::new` is built knowing whether it `can_enrich` and
  the settings pane knows what to draw. A scan that was not cancelled runs `enrich(false)` as it
  ends, where `can_enrich` — online *and* a reference — allows it, and the lookup's own task polls
  at `SCAN_POLL` and reloads every `POLLS_PER_RELOAD` polls the way the scan's does, so rows and
  covers arrive under the panes as the reference answers. Turning Online off under a run does
  not take the reference away; it stops `enrich` starting, and `has_reference` is what the card
  reads to say a run started offline has nothing to reach until the next start.
- **A file changed under a root while the window runs is followed on its own, and a file taken
  away is forgotten at once rather than scanned for.** `resonate_library::RootsWatch` is a
  recursive inotify watch over the roots, through `notify`, and it sorts what it hears two ways.
  A path *gone* — an audio file or a folder removed, or anything but a sheet renamed away — is
  noted by path and handed out by `taken_away` once `GONE_QUIET_FOR` has passed since it was last
  heard, so a tagger that deletes and rewrites a file in one breath is seen standing again — and
  never while a change under the roots is still settling, because a rename is a path gone and a
  path changed at once, and the scan the change sets off is what follows the file to where it
  went rather than forgetting it and counting it anew. A path
  *changed* — an audio file or a sheet made, written or renamed in, a folder made, a sheet taken
  away — notes its root, and `settled` hands the root out once it has been quiet for
  `ROOTS_QUIET_FOR`. An inotify queue that overflowed notes every root. A catalog or a vault kept
  inside a root cannot set off the scan that writes it, because neither writes audio or sheets
  there. The model's `watch_the_roots` looks every `ROOTS_LOOKED_AT_EVERY`, a quarter second: it
  rebuilds the watch wherever `Library::roots` moved, hands what is gone to
  `Library::forget_the_gone` — which takes the `Walk` guard, deletes the rows at each path or under
  it whose file is not there, spares a vaulted row and a delivered one the way the scan's prune
  does, and sweeps what that orphaned — and reloads where it forgot anything, and where a root has
  settled and nothing holds the work slot it runs the ordinary incremental scan of that root,
  `Prompted::OnItsOwn`. A forget refused because a scan or an import holds the guard is kept and
  tried on the next look, so a file deleted mid-scan leaves the list the moment the scan ends
  rather than after another quiet period and another whole-root scan. What this replaced waited
  for the root to be quiet and then rescanned all of it for every deletion, so deleting files one
  after another kept pushing the scan back and a large root took its whole walk to drop one row.
  A watch that cannot be made — inotify out of watches — is a warning, and that root waits for a
  scan by hand.
- **A browse pane holds a window onto the listing, and every count comes from the catalog rather
  than from the window.** `LibraryModel::reach` is how many rows `browsed` asks for — `PAGE` of
  2 000 to begin with — and `reach_further` grows it by another page when a list has drawn to
  within `LOOK_AHEAD` of what it holds *and* holds everything it asked for, which is what says
  there may be more. Growing re-reads with a larger limit rather than appending a page at an
  offset: the sort is redone either way, and a window that is always a prefix of one ordered read
  cannot duplicate or skip a row the way a second read at an offset can under a scan. It is called
  from inside the `uniform_list` processors, which is the one place that knows how far a list has
  been drawn; `held < reach` is the re-entrancy guard, because a growth in flight leaves the window
  larger than what is loaded until it lands. `LOOK_AHEAD < PAGE` is a `const` assertion rather than
  a hope: a look-ahead past the page reaches the end of the *first* window on the first frame and
  the window grows itself to the whole library unasked. `set_query` and `select` put it back to one
  page, because the rows under the old window are not the rows a new query names.
- **What a count says is what the query matches, not what the window holds.**
  `Library::albums_counted`, `artists_counted` and `measured` answer over the whole match — the
  first two sharing their scoping with the listings through `narrowed_onto`, so a count and a
  listing can never disagree about what the search means — and `Measured` carries the rows, the
  total length and how many of them are lossless, the last from a `tracks.codec IN (…)` built off
  `Codec::ALL` filtered by `is_lossless`, so the SQL and the Rust cannot drift. The sidebar's three
  figures and the tracks heading's summary read those rather than `Vec::len`, which used to report
  2 000 for any library larger than that. `measured` still honours a limit it is given, because a
  saved query's cap is part of what that query *is*; the window passes `limit: None`.
- **A gesture over "every track listed here" reads the whole listing before it acts.**
  `RootView::with_everything_listed` reads the unlimited `TrackQuery` that `listing_whole` builds
  on the background executor and hands the rows to a closure, so *Play*, *Play next*, *Add to
  queue* and *Add to playlist* in the tracks heading act on everything the search matches rather
  than on the window that happens to be loaded. It is a task on `RootView` rather than a blocking
  read, because the listing may be the whole library.
- **A scope is the tracks pane's alone, and the sidebar counts the library rather than the scope.**
  `Selection` narrows one listing and no other: `browsed` reads the albums, the artists and the
  tracks the search allows, and reads a *second*, narrower track listing beside them only where an
  album or an artist is scoped. `LibraryModel::tracks` is the first and `LibraryModel::listing` is
  what the tracks pane, its heading, its summary and its *Play*, *Play next*, *Add to queue* and
  *Add to playlist* all draw from, so the sidebar's three counts say what the library holds however
  deep a listener has gone into it. What it replaces is one listing narrowed in place: opening an
  artist took its albums out of the albums pane and its count down to that artist's, opening an
  album took the tracks count down to that album's, and nothing on screen said why. The second read
  is what it costs, and it is the cheap one — an album's dozen rows, not the library's two thousand
  — which is why the whole listing is the one that is always read.
- **A scoped heading names its artist as a line of its own, and its cover magnifies.** The album
  hero draws the owner between the title and the summary rather than folded into the summary's
  first field, because a name run together with *1973 · 10 tracks · 43:12* cannot be pressed at
  only its own width; it goes through `RootView::opens` like the playback bar's, so the gesture and
  the hint are written once. The scoped cover carries `COVER_HINT` and `RootView::magnify`, so the
  album being read is as magnifiable as the one playing. An artist's portrait is not: `Magnified` is
  an album or a file, and a portrait is neither.
- **An album's rows are what the catalog holds merged with what the release says is missing.**
  `LibraryModel::rows` is an `Arc<[AlbumRow]>` built by `album_rows` whenever the selection is an
  album: `AlbumRow::Held` indexes the tracks listing and `AlbumRow::Missing` the release rows
  whose `track` is `None`, and the two are sorted together by `(disc, position)` — a held track at
  the place its paired release row gives it, or at its own disc and number where nothing paired
  it, a number of none sorting last — so the tracks pane's `uniform_list` counts `rows` inside an
  album and `tracks` everywhere else. That seat order is drawn while the pane's sort is the
  album's own, `Relevance` or `AlbumThenTrack`, turned round where it reads backwards; any other
  sort the heading's *Sort* picks draws the held rows in that sort and what the album lacks after
  them, and `arranged` is that choice. **What plays is what is drawn**: a row's click and Enter go
  through `LibraryModel::played_from`, which queues the `Held` rows in the order `rows` holds
  them, and *Play*, *Play next*, *Add to queue* and *Add to playlist* put the whole listing they
  read through `AsDrawn::ordered`, the same arrangement over the same release rows — so the row
  under the pointer starts and what follows it is the rows under it on screen. `unheld_row` draws faint in the same eight cells, the
  number, title, artist and length off an `Unheld` — built `From` a `HeldReleaseTrack` here and
  `From` a `MissingTrack` in the Missing pane, so one row serves both — and in `controls_place`,
  the width a held row's controls take, one mark: `Icon::Want` sending `LibraryModel::want` or
  `Icon::Wanted` sending `unwant`, both through `edit` like a playlist gesture — `want` through
  `edited_then`, which asks the providers once the want is written — with
  `Loaded::wanted` mapping each `ReleaseTrackId` to its `WantId` so the mark knows which it is.
  Neither the grid's caption nor the scoped heading counts what an album is short of any more:
  the Missing pane, the inline `unheld_row`s, the sidebar figure and the artist heading's
  *N releases not held* each say it once where it is the subject, rather than on every cell in a
  library. Under the subtitle ride `release_line` — date, label, catalogue number, country and kind,
  whichever the release has — the disambiguation in the faint colour, and `heard_on`, one faint
  line naming the services out of `service_names`, which leaves `Service::Other` out; an artist
  hero draws `profile_line`, up to `GENRES_SHOWN` genres as `kit::tag` pills and the same
  services line, and at the end of its actions, where `ArtistDetail::releases_unheld` is above
  nothing, a `Tone::Ghost` button reading *N releases not held* under `UNHELD_HINT` that opens
  `Pane::Missing`. **The services are capped at `SERVICES_SHOWN`, because an artist has a
  directory of them.** A release names a handful of shops; an artist named nineteen — every
  streaming service, both encyclopaedias and four social networks. Four is what is drawn, the way
  three genres are, and as one line of words rather than a cloud of mono figures. **Each service
  on the line is the link it was named from.** `service_names` keeps the first URL a service is
  linked by beside its `Service::title` — *Apple Music*, *SoundCloud*, not the lowercase key the
  catalog stores — and `heard_on` draws one pressable name per service, lit under the pointer the
  way an `opens` is, naming *Open on …* and handing that exact release's or artist's URL to
  `cx.open_url`, which is the desktop's browser. It stops its press and ignores a right one, like
  every link in a heading.
- **The artists pane is a list or a grid, and the heading chooses.** `ArtistsDrawn` is `List` —
  the rows it always was, a small portrait beside each name — or `Grid`, the albums pane's shape
  with the artist's portrait in place of a sleeve: `artist_grid` measures the same `grid_width`,
  reads `grid_columns` and lays out rows of `artist_cell`s at `theme::grid_cover()`, each a round
  `portrait_frame` read at `Portrayed::InAGrid` — or `kit::avatar_at` the same size, the initial
  scaled with it, where no portrait is held — over the name and its counts, favoured, pressed and
  menued as a row is. The choice is a `kit::segmented` of *List* and *Grid* beside *Sort* and
  lives for the run like the other listings' orders. The reach keys serve both: in the grid a
  page is whole rows of cells, `show_row` scrolls the grid row holding the artist and a reached
  cell wears the same `reached_ring` a reached album does.
- **An artist's page is its albums or its tracks, one at a time, chosen from two tabs.** It used
  to stack a horizontally scrolling strip of small covers over the whole track listing, so a
  page was three scrolling regions, an album was a thumbnail and every row repeated the artist's
  name. `artist_tabs` is a `kit::segmented` of *Albums* and *Tracks*, each with its count, and
  `RootView::artist_shows` is the `ArtistShows` it chose — kept for the run and put back to
  `Records` whenever `RootView::opened` opens an artist. *Albums* is `artist_records`: every album
  the artist owns or plays on as a wrapping grid of `album_cell_captioned` cells at the grid's
  own `theme::grid_cover()`, scrolling under an id keyed by the artist so the next artist opens
  at its top. `Caption::Beside` is what the cells are captioned with — the year and the track
  count, and the owner only where it is somebody else, which is what an album the artist merely
  plays on needs to say. *Tracks* is the ordinary listing under the ordinary header, and it is
  the only tab that offers *Sort*, a sort having nothing to put in order under the other.
  `ArtistShows::within` answers *Tracks* for an artist who holds no album at all, and then no
  tabs are drawn. *Play*, *Add to queue*, *Play next* and *Add to playlist* read the whole
  listing either way, because they act on the artist rather than on the tab.
- **The Missing pane is what the catalog knows it is short of, headed by run, one half at a
  time.** `Pane::Missing` sits under `Section::Collection` beside the playlists and its sidebar
  count is `Missing::tracks`, hidden at zero, so a library the reference has never described
  carries no figure for nothing. The two halves are two lists chosen from two tabs, the way an
  artist's page chooses between its albums and its tracks: `missing_tabs` is a `kit::segmented`
  of *Tracks* and *Releases*, each with its count, and `RootView::missing_shows` is the
  `MissingShows` it chose, kept for the run. `MissingShows::within` hands over to the half that
  holds something where the chosen one holds nothing, and the tabs are drawn only where both
  do; the artist heading's *N releases not held* opens the pane on *Releases*. They used to run
  on in one list, where an artist's heading after the last album's rows was indistinguishable
  from another album's. Each tab is one `uniform_list` over `LibraryModel::missing_track_rows`
  or `unheld_release_rows`, `Arc<[MissingRow]>`s of `Album`, `Disc` and `Track`, or `Artist` and
  `Release`, indices that `models::missing_track_rows` and `unheld_release_rows` build: a
  heading wherever the key changes and a row for every entry, which
  `each_run_of_an_albums_missing_tracks_is_headed_by_the_album_once` and
  `each_run_of_an_artists_unheld_releases_is_headed_by_the_artist_once` are the claims of.
  The artists' half is `headed_by_run` over one key; the albums' half is
  `headed_by_album_and_disc` over the album *and* the disc, because a set's missing rows number
  from one again under each disc and a single heading over the lot reads as one album with two
  track ones. A disc heading is drawn only where the album's own run spans more than one, the
  same rule `headed_by_disc` follows inside an album, and
  `an_album_missing_rows_from_two_discs_is_headed_by_each_of_them` is the claim; it is an
  eyebrow reading *DISC N* under the title column and no more, the pane holding no
  `HeldMedium` to name a format or a title with. **A run is a card, drawn a row at a time, because every row of a
  `uniform_list` is one height.** `Place::of` reads where a row stands in its run — the
  heading is `Head`, the row before the next heading or the end is `Last`, everything between
  is `Within` — and `in_a_card` draws that slice of a `kit::section`-like card: the heading on
  the `raised` ground with the card's top edge and rounded top corners, the rows on `surface`
  with a hairline under each, and the last one closing the card with its rounded bottom
  corners. The gap between two cards cannot be a margin, so it is taken out of the rows that
  border it: the heading and the last row are each `HALF_BETWEEN_CARDS` shorter than the row
  they stand in, the heading sitting at its row's foot and the last row at its head.
  `a_run_is_one_card_opened_by_its_heading_and_closed_by_its_last_row` is the claim. Rows
  laid loose on the pane under a heading band of their own read as text floating with nothing
  holding it together. `run_band` is what the heading holds: the album's cover or the artist's
  portrait — `kit::avatar` where there is none — in the number column, the name through
  `opens` in the semibold text colour, the owner beside it in the muted one, and how many the
  run holds — *7 missing*, *11 releases* — ending where the lengths end. A track row is `unheld_row` under `Beside::ARun`, which drops the
  format and plays cells, the pane having no column header for them to line up under; a
  release row is `release_row`, its title in the muted colour, its kind a `kit::badge` and its
  first release year in the length column, and no press, because a release the catalog holds
  nothing of names nothing it can show. The subtitle speaks for the tab in front — *8 tracks
  missing from 2 albums*, *46 releases by 8 artists not held*. `Loaded` reads `missing_tracks`
  and `unheld_releases` under `MISSING_AT_MOST`, 5 000, and `missing_counted` on every load,
  so the tracks and releases the subtitle and the tabs count are the catalog's rather than the
  window's; the albums and artists beside them are the headings the list drew.
  Empty, it is `kit::empty` under `Icon::Missing` saying nothing is missing, and where the build
  `can_enrich` a second sentence says where the answer would come from.
- **A search lists what the library is short of after what it holds, greyed the way an album's
  missing rows are.** `LibraryModel::rows` is a `ListedRow` for every selection now, not only an
  album: under *All tracks* with a search it is the held rows followed by
  `ListedRow::Beyond(Beyond::InTheCatalog(n))` and the `Unheld` rows `Library::unheld_matching`
  answered — release rows the catalog knows it lacks — then `Beyond::Elsewhere(n)` and the `Found`
  rows MusicBrainz answered, or `Beyond::Asking` while it has not. `beyond_the_listing` builds it
  and answers nothing until the listing is whole, so a paged listing never draws its later pages
  under the sections, and an empty `rows` still means the listing drawn one row per track, which is
  what `played_from` and `listed_rows` read. Both kinds of row are `unheld_row` with a
  `Beside::ASearch` — a cover column, the album cover faded to `UNHELD_COVER` or a dashed
  `Icon::Missing` frame where there is none, the matched runs lit, the release in the format
  column — and the want mark is `want_mark` over an `Asks`: a catalog row wants its
  `ReleaseTrackId` as ever, a found song calls `LibraryModel::want_found`, which lands its release
  and wants the row on the background executor, greys the mark while it does, then asks the
  providers and asks MusicBrainz again, so the song moves up into the catalog's section wanted.
  `wanting` holds a task per recording rather than one for the lot, so wanting a second song
  before the first has landed does not drop the first's lookup and leave its mark grey.
  The ask is `ask_elsewhere_after`, `ASKED_ELSEWHERE_AFTER` — 700 ms — behind the keystroke, only
  where the build `can_enrich` and `asks_elsewhere` says the words are worth it, and its answer is
  kept against the text it answered, so a reload does not ask again. Going back to a text already
  answered cancels whatever ask was in flight, and an answer is stored only while its text is
  still `asking`, so an ask outrun by the box never writes over the songs found for what it now
  says. A search whose plain words a
  held track sings is offered as `lyrics:"…"`: `Library::sung` rides in the load, and *Sung in N
  tracks* stands in the heading's actions, and in a pane's empty state where nothing else
  matched, as a `search_instead`.
- **The search box narrows the Missing pane, and the two halves answer it differently because
  one is in the catalog and the other is not.** Both reads take the query the browse panes take,
  so the *Reads* row stands under this heading too and the counts, the sidebar figure and the
  list can never disagree; `library.md` has the SQL. A missing track is narrowed by the *album*
  it is short from, through the catalog's own grammar, so typing an album, its owner or a track
  it holds brings up what that album lacks. An unheld release is in no catalog and has only its
  own row to answer with, so it is narrowed by the fold of its title, its kind and its first
  release date, or by the artist it is under — which is what narrows a prolific artist's
  discography by year and by kind without a control per field. Narrowed to nothing the pane
  says *Nothing missing matches* rather than that nothing is missing, and offers no lookup,
  the same branch the browse panes take.
- **The playlists pane is an index and one opened playlist, not a `Selection`.** `Selection` names
  Everything, an album or an artist, and a playlist is `LibraryModel::opened` instead, with its
  entries riding in the same `Loaded` snapshot as the albums, artists, tracks and roots — so nothing
  else has to know when a playlist changes. Rows are put in one through
  `RootView::hold_for_a_playlist`, the picker that the tracks pane's `+`, the tracks heading's *Add
  all* and the queue's *Save as a playlist* all open. What the picker offers is a second listing
  riding in that same snapshot — `Library::playlist_lists`, read unnarrowed and holding only the
  playlists that hold a list. Unnarrowed because the gesture is most often made from a search and a
  track found by name must still reach a playlist whose name has nothing to do with it; lists alone
  because a saved query refuses rows anyway, which is what takes the `count(*)` per saved query off
  the read. Riding in the snapshot is what leaves the picker with nothing to read on the frame it
  opens and puts a playlist started while it is open on it, so `adding` carries the `Held` and
  nothing else. `Offered` is the arithmetic over it: the playlist the rows came out of is stepped
  over by position rather than filtered out, so a `uniform_list` draws the rows —
  `theme::PICKER_ROWS` of them at a time — without a copy of the listing per frame.
  `RootView::show_playlist` is how the pane opens one, and it takes the `UniformListScrollHandle`
  `left_at` holds against that playlist's id, so returning to a long one lands where it was read to
  rather than at its top. `set_query` puts a fresh handle in whenever a keystroke narrows the rows
  differently, because the rows under the old offset are no longer the ones it was left on, and
  `forget_what_has_gone` drops the handle once the listing stops naming its playlist — passing over
  a listing a search has narrowed, because what that leaves out is still there. None of it survives
  the run, and neither does the settings pane's category, the listing's order or the pane the
  sidebar was on.
- **One `Field` names every playlist.** `RootView::name` is shared by the pane's new-and-rename row
  and the picker's *or a new one*, so only one of the two can be open: `name_a_playlist` closes the
  picker and `hold_for_a_playlist` closes the naming row. The field emits `Submitted` on enter and
  `RootView::name_given` decides from whichever is open whether the name creates, creates-and-adds,
  renames, or saves and revises a search. Which heading draws the row is the `Naming` variant's:
  `Naming::Query` is drawn under the *tracks* heading, where the gesture is and where the search box
  above it is still live, and `New` and `Rename` under the playlists heading — one entity cannot be
  in the frame twice, so each heading filters `naming` for the variants the other leaves.
  `RootView::contact` is the third `Field`, the Online card's, and `Field::hold` is what fills it:
  `set_content` with the preedit cleared, the shape `clear` already had, so the saved contact is
  in the box when the window opens and is put back trimmed on enter. `contact_given` stores
  `Setting::Contact`, moves the global, reports that it is sent from the next start and hands
  focus back; escape and a press outside leave it through `leave_contact`, ahead of the naming
  row in `dismiss_search`, and `editing` counts it so the typed keys stay off while the caret
  is in it. The order
  and the cap ride beside the name as chips, because a saved query is a search, an order and a row
  cap and the window would otherwise only ever save the first. *Edit search* on a saved query is
  `RootView::revise_search`, which puts the query's text back in the search box, makes the pane
  Tracks and fills the chips from what was saved, so revising one is the gesture that saved it.
- **A run of rows is dragged where it goes, and the keyboard reaches one before it moves it.**
  `views/reorder.rs` owns the whole of how rows move: `Shift` names which list is being edited,
  `Step` is up or down and `Step::landing` is the one piece of arithmetic behind the arrows, the
  drop and the keys alike, which is why it is what `resonate-ui`'s own tests cover — it steps off
  `Span::first` going up and `Span::last` going down, so a span and a row move by the same rule. A
  row is both a drag source and a drop target, so dropping a span on row `to` is the `Command::Move`
  the arrows already sent — take out and put back, never a swap. The payload is `Carried` and the
  `Ghost` under the pointer is the first row's title and how many more ride with it, padded by the
  offset gpui hands the constructor so it rides where the pointer is rather than where the row
  began. `RootView::reach` is a `Reach`: a `Shift`, the anchor the reach opened on and the row it
  has moved to, so a reach left in the queue is not drawn in a playlist, and it lives only as long
  as the run. `up` and `down` move it and collapse it — the queue opens on the playing
  row, a playlist on its first — `shift-up` and `shift-down` grow it from the anchor, `home` and
  `end` take it to either end, `pageup` and `pagedown` move it by `rows_a_page` — the list's own
  viewport height over `theme::row_height()`, read off `UniformListScrollHandle`'s `last_item_size`,
  whose `item` field is the viewport and whose `contents` is the whole list — `ctrl-a` reaches every
  row, `shift`-click reaches every row between, `alt-up` and `alt-down` move whatever is reached with
  the reach following it, `enter` plays its first row and `delete` takes the reached rows away
  through the same `drop_rows` the ✕ calls, gated on `Rows::are_edited` in a playlist the way the ✕
  is. `backspace` over a reached row drops it too rather than silently editing the search, which is
  what `typed` did with it before: `drop_reached` answers whether it acted and the query is only
  edited where it did not. `RootView::acting_on` is what every row
  gesture asks: the reach where the pressed row is inside it, that row alone where it is not, so the
  ✕, the arrows and the queue marks need no notion of a selection of their own. It is drawn as a
  2-unit accent edge and a faint accent wash that every reorderable row reserves in
  `theme::UNMARKED`, so the mark costs no layout when it moves. A saved query draws none of it, because its order is the query's. A drop is
  the one gesture that asks `acting_on` nothing: `reorder::movable` takes the row it stands for
  beside the `Carried` it would hand over, because where a span lands is the row the pointer let go
  over and not what that row would have carried had it been the one dragged.
- **A drag scrolls the list, and it is a held pointer that keeps it scrolling.**
  `reorder::follows_a_drag` puts an `on_drag_move` on the div wrapping each `uniform_list` and
  hands `RootView::creep` a `Creeping` — which way, whose scroll handle and how many rows — while
  the pointer is within `DRAGGING_EDGE` of an edge, and `None` where it is not. gpui reports a drag
  only when it moves, so the move alone would stop the moment the pointer settled: `creep` scrolls
  one row past `ScrollHandle::top_item` or `bottom_item` and then runs `creeping_on`, a task that
  does it again every `DRAGGING_STEP` until `creeping` is cleared or `App::has_active_drag` says
  the drag is over. One task serves both ends and both panes, because it reads `creeping` each turn
  rather than capturing it, and it is the one that clears the cell, so a live cell and a live task
  mean each other.
- **The settings pane is a rail of categories over a column of sections, and one table says what a
  section is.** `views/settings/` is a folder rather than a file, because the pane now holds two
  pieces of arithmetic worth testing without a window. `find.rs` is the vocabulary: a `Category` —
  Output, Processing, Equaliser, Library, Online, Desktop, Appearance, About — and a `Group`, one per setting, carrying
  its category, its title, its hint, the words it is *also* called and the `SettingKey`s that put it
  back. Nothing else enumerates the sections: `Category::groups` filters the one table,
  `mod.rs::group` dispatches a `Group` to the method that draws it, and tests hold every group to
  standing in one category, to a title nothing else claims, to a hint of its own and to no key being
  claimed twice. The rail is `theme::settings_rail()` of icon rows down the pane's left edge,
  `RootView` remembers which is open for the run, and the body is a column of `kit::section`s —
  a header strip carrying the title, the revert mark and the info mark over a body — rather than
  the flat cards the pane used to draw. The body scrolls under `RootView::settings_scroll`, a
  `ScrollHandle` of its own, and `show_settings` puts it back to the top whenever the category
  changes: gpui keys a scroll offset by element id, and one `"settings"` id across every category
  left the next page opened as far down as the last one had been read.
- **Discord is two Desktop groups, and they write a global before a file.** *Discord* is the
  switch and the application id; *What Discord shows* is how much text, which picture, the icon,
  the bar and whether a pause keeps it. Each control changes `ResonateApp::presence`, stores its
  `Setting` and hands the whole `Presence` to `Present::follow`, so the publisher starts, stops or
  re-sends at once. The id and the icon are `Field`s read on enter through `AppId::parse` and
  `Icon::parse`, and a text neither reads is a `Notice::Trouble` that stores nothing rather than a
  key the next start refuses; a blank one clears the key.
- **The body is as wide as a number, never `w_full` under a `max_w`.** taffy fixes a flex item's
  height from a measure taken before a percentage width has resolved, so a column clamped that way
  is measured with its text *unwrapped*, its `content_size` comes out short and the bottom of a long
  category cannot be scrolled to. It is the same trap the lyric row carries, and the answer is the
  same: reach for a pixel wherever a column's height depends on how its text wraps.
- **A boolean is a `kit::switch`, a choice is `kit::segmented` and only a device or a palette is
  still a row.** The four near-identical two-variant enums the pane used to carry are gone: a switch
  names what it does and what off means and takes the whole row as its hit area, and a `Choice` is
  a trough of segments. `kit::section`, `kit::field`, `kit::segmented`, `kit::switch`,
  `kit::preview`, `kit::dot_swatch` and `kit::choice_row` with its `kit::radio` are the
  vocabulary, and the pane draws none of them itself — the same rule `kit::button` has always had.
  **`kit::segmented` answers with the trough itself and the caller is what hugs it.** It used to
  answer with a wrapper around the trough, so every segment landed beside the trough rather than
  inside it: the choice drew as bare labels with an empty six-pixel pill standing to their left, in
  every selector the settings pane has. A builder that returns a parent its children will not
  reach is the shape to watch for.
- **A device reads as what it is and what it takes, and its node name is behind the pointer.**
  Each choice, *Follow the system default* included, is a `kit::choice_row`: a `kit::radio` held
  `kit::level_with_the_choice` beside a column whose first line is a `kit::choice_name`, so the
  radio sits on the name rather than midway down a row of three lines and the default entry and
  every device line up on one edge. The chosen row takes the whole accent as its border over an
  accent wash, the way the worn palette card does, and that and the filled radio are the only two
  marks — the speaker every row led with and the 2-unit accent edge the chosen one wore were a
  third and a fourth way of saying the same thing, and the speaker, identical on every row, said
  nothing else. The name is `flex_1` and ends in an ellipsis — sized by its content instead, a
  clamped name collapsed to nothing beside its badges, and `truncate` alone sliced its last
  letter — and the badges ride the same line after it — `DEFAULT`, `UNPLUGGED`, `IN USE` and the verdict on the playing track, `TAKES … AS IS`
  or `CONVERTS` — so however long a description runs they cannot be pushed off the end, and the
  verdicts down the list read as one column. Under the name are two `kit::details` lines, each a
  run of parts divided by a faint `·`: how the sink is reached, in words — the port, the profile
  the card is switched to and whose volume it is — as faint `kit::detail`s, and what it takes, in
  figures — the depths it offers and the rates it allows through `format::depth` and
  `format::kilohertz`, listed with commas — as `kit::figure`s. What it replaces drew all five as
  identical grey mono badges in one wrapping cloud, put the words in the figures' face, and listed
  both with a slash that read as a shorthand. A port with
  nothing plugged in is a badge and a name drawn `muted` rather than a parenthesis after the port,
  and the default entry says which device PipeWire is routing to now where the graph names one.
  The node name is still what the settings file holds, so it is what the row `names` itself, and
  `SinkInfo::is_hardware` is drawn nowhere because it reads `device.api` off the node, which an
  ordinary ALSA sink does not carry.
- **A section holds a subject rather than a control.** The theme shelf and the accent swatches are
  one *Colour* section with a `kit::field` label over each half, because six sections of one control
  each read as a list of switches rather than as a page. A group's hint is written once, in the
  table, and the info mark in its header is the only place it is drawn — `views/hint.rs` is the view
  behind it, because a gpui tooltip is built from an `AnyView` and not from a string.
- **Every group ends in a line that says what it does, and a choice says what the chosen option
  does.** `Choice::meaning` is required of every choice rather than defaulted, so a choice cannot
  be added without one, and `RootView::choices` draws the chosen option's meaning under its
  trough as a `note` — the resampler's filter, a dither, a ReplayGain mode, a buffer depth, a
  Discord picture — while `detail` stays the hover for figures such as the filter's taps. The
  pages had grown half with a closing line and half ending at their controls; a group that is a
  list or a row of buttons — the devices, the music folders, the scan, the bindings, the three
  About readbacks — ends in a `note` of its own, and every note is the one `note` builder at
  `text_sm`, the accent line included. *Lossy sources* is *Artificially enhance lossy files* now,
  carrying the `EXPERIMENTAL` badge `Group::is_experimental` puts in a section's header, because
  what it adds is a guess at what the encoder threw away; each of Off, Repair and Repair and
  extend says in words what it takes or adds, and `LOSSY_ONLY` under them says which files are
  touched and that a repaired one is no longer bit-perfect.
- **A palette is shown rather than named.** `kit::preview` is a strip of four bands — the theme's
  sidebar, its panes, what it raises above them and the accent that would be worn — under the
  palette's name inside a card that takes the accent as its border when it is the one worn. An
  accent is a round `kit::dot_swatch` carrying a check in whichever ink `theme::ink_over` says reads
  on it. Appearance is the one category with nothing behind it anywhere but the window: a choice is
  worn through `theme::wear`, saved through the `Settings` seam and drawn again by
  `cx.refresh_windows()`, so no `Command` is sent and the pane marks what is worn rather than what it
  last sent. Text size goes the same way and moves every measure with it, and *Window buttons* is
  the third group there — the chrome is part of how the window is drawn, where the Desktop category
  is what the player tells the rest of the session — worn through `RootView::show_window_buttons`
  onto the global rather than through `theme`, because a button is not a colour or a measure.
  *The volume wheel* is the fourth, one switch worn onto `ResonateApp::scroll_volume` the same way
  and written as `scroll-volume`, *Scrollbars* the fifth, a choice of Always, While scrolling
  and Never worn onto `ResonateApp::scrollbars` and written as `scrollbars`, and *Sidebar tabs* the sixth, three switches worn onto
  `ResonateApp::tabs` through `RootView::show_tabs` and written as `suggestions-tab`,
  `missing-tab` and `tab-counts`, the last taking the figure off every tab the sidebar draws. `Pane::is_shown` is what a tab being off means: the sidebar and `stepped_pane`
  pass the pane over and `set_pane` lands on the tracks instead, so a way back or a button cannot
  reach a pane the listener hid, and hiding the pane in front moves off it at once.
- **gpui scrolls a region and draws no bar for it, so `views/scrollbar.rs` does.** `Scrollbars::of`
  reads the setting once where a pane is built, and `vertical`, `horizontal` and `around` answer
  a bar over the region's edge — or an empty absolute div where the mode is `Hidden`, so a pane is
  built one way either way. A bar reads the region's own `ScrollHandle` — a `uniform_list`'s base
  handle — and paints the thumb in a `canvas`, because the offset moves between renders and only
  paint sees where it is now. **Nothing a bar knows survives a render**: a `Cell` made in the
  builder is a new one each frame, so a hover written into one by `on_hover` was gone by the
  redraw it asked for. The bar lights by weighing `Window::mouse_position` against its own bounds
  in paint, and the paint writes those bounds into a cell the same frame's listeners read, so a
  press on the track and a drag's grip measure the bar the pointer is actually over. Where on the
  thumb it was held is taken when the drag *starts*, in `on_drag`'s constructor, rather than on
  the press, because a press on the track moves the thumb and redraws before the drag begins. **The
  lyrics pane's bar is drawn only while the sheet is moved.** A sheet that follows the song glides
  on its own for its whole length, and a thumb standing beside the words the whole time read as a
  second, busier line to watch; `Scrollbars::vertical_while` paints the thumb only where the pane
  says it moved — `LyricsModel::moved_by_hand_lately`, `BAR_LINGERS` after a wheel turned it — or
  where the pointer is on the bar or its thumb is held. The glide is not a movement for this, and
  `is_turning` keeps the frames coming until the linger is out, so the thumb goes away on its own.
  **`ScrollbarMode::AutoHidden` does the same for every bar, from the offset alone.** A bar that
  would always be drawn is drawn `Shown::WhileScrolled` instead: its paint keeps the last offset
  it saw in element state under `LAST_SCROLLED` — the one thing a bar carries from frame to frame,
  because a `Cell` would not survive, and read on every paint whether or not the pointer already
  lights the bar, because gpui drops a state no frame reads — and paints the thumb while that offset moved within
  `SCROLLED_LINGERS`, or while the pointer is on the bar or its thumb is held. The paint that
  sees the offset move spawns one timer that refreshes the window once the linger is out, so a
  thumb goes away without frames being asked for in between. The lyrics bar keeps its own rule
  under every mode but `Hidden`, because the glide moves its offset and is not a scroll.
- **A setting that has moved off its default says so, and the mark is what puts it back.**
  `defaults.rs` is the arithmetic: `Standing` is what is worn — the `OutputSettings`, the
  `Appearance`, the two online flags and whether a contact is given — `differs` weighs it against
  `EngineConfig::default()` and `Appearance::DEFAULT`, and `puts_back` answers with the commands
  that restore it. A section whose value differs grows an `Icon::Undo` in its header; the footer
  carries *Reset <category>* for every category that has a key to put back, and About's *Reset
  everything* arms on the first press and fires on the second, because a modal is not in this
  crate's vocabulary. Putting a setting back is two moves, not one: the default is sent and the key
  is *taken out of the file* through `Settings::forget`, so a build whose default later changes is
  followed rather than pinned to the old one. A group with no key — the folders, the scan, *Look up
  now*, and all three of About's — offers no mark, which a test holds.
- **A setting is found by typing rather than by remembering which tab holds it.** `RootView::finding`
  is the pane's own `Field`, reached by `ctrl-,` or by a press, counted by `RootView::editing` so
  the transport keys stand down while the caret is in it, and cleared-and-blurred by escape ahead of
  the contact field in `dismiss_search`. `Narrowing` matches every word of the query against a
  group's title, its hint, its category and the words it is also called — `sink` finds the device,
  `latency` the buffer, `sacd` DoP — so a setting is reachable by what another player calls it.
  While a query is in force the heading reads *Found*, the rail draws each category's match count
  instead of a selection, and the body draws the matching sections from *every* category under a
  category eyebrow, because a filter that only looked inside the tab in front would be a worse
  version of the tab.
- **A right press answers with a menu, and `views/menu.rs` is the whole of how.** `Menu` is a
  position and a run of entries, each an icon, a label, an optional key and a closure;
  `Menu::at(…).does(…).under(…).apart()` builds one and `menu::opens_a_menu` attaches it to any
  element, so a pane says what a press offers and never how a menu is drawn. It renders as
  `deferred(anchored().position(at).snap_to_window_with_margin(…))` over an occluding scrim that
  takes the outside press, and it is one `Option<Menu>` on `RootView` with one arm at the head of
  `dismiss_search` — the shape `adding` and `magnified` already have. It is not built on
  `tooltip`, which `hint.rs`'s source-walking test forbids and which neither a press nor a scroll
  takes down. It needs no key context of its own: `up`, `down` and `enter` already name actions
  this window handles, so `reach_row` and `play_reached_rows` step and press the menu before they
  reach for a row.
  **`on_click` fires for a right press too**, which is the trap the whole thing turns on — every
  row would have played, scoped or seeked on the press that opened its menu. `menu::pressed` is
  the one reading that says otherwise and every listener sharing an element with a menu takes it.
  A track row plays, queues either way, holds for a playlist, reaches the artist and the album,
  shows its file in the file manager and copies the path — *Go to album* being the gesture with
  nowhere else to live, a track row having no album column. `Menu::offers_the_file` is the last
  two, written once for the tracks, queue and playlist rows alike: *Show in the file manager* is
  gpui's `reveal_path`, which asks the portal's `OpenURI.OpenDirectory` for the folder with the
  file selected and falls back to opening the folder. An album cell, an artist row, the two covers, a queue row, a playlist row, a
  column header, the search box and a lyric line each carry their own. A sidebar row, a settings
  control and a missing row deliberately carry none: each would offer only what one press already
  does.
- **A copy is handed to the compositor over `ext-data-control`, not through gpui.** gpui 0.2.2's
  Wayland `write_to_clipboard` sets the selection under the serial of the last *key* press, so a
  copy made from a menu with the pointer — the path, a lyric line, a share — carried a serial of
  nothing or a stale one, and KWin kept whatever the clipboard held before. `clipboard::copy` is
  the one way the window copies: it opens a connection of its own, binds
  `ext_data_control_manager_v1` and the seat, sets a source offering the text types and waits
  `ANSWERED_WITHIN` for the round trip that says the compositor took it, then a thread named
  `resonate-clipboard` answers each `send` with the bytes until the source is cancelled — which is
  what the next copy, ours or anyone's, does, so one such thread is alive at most. Where the
  compositor offers no data control — GNOME keeps it to privileged clients — the copy falls back
  to gpui's own write, which works whenever the window was last reached by the keyboard. It is
  `wayland-client` and `wayland-protocols` with `staging`, both already linked by gpui, and no
  `unsafe`: the descriptor arrives as an `OwnedFd` and is written through a `File`.
  It was proved against a headless `kwin_wayland --virtual` on a socket of its own, read back with
  `wl-paste`, and is not a test, because a copy in a desktop session is the listener's clipboard.
- **A scope remembers where it was opened from, and its way back says where that is.** `Wayback`
  is the pane, the selection, the row the list was left on and the name of the place, in a
  bounded `WAYS_BACK` stack, and every gesture that scopes goes through `RootView::opened` so it
  is written once rather than at each call site. The name is read by `RootView::here` as the
  scope is left — the album's title, the artist's name or the pane's label — because by the time
  the way back is drawn the library has moved on to the new scope and could no longer say it.
  The way back is `kit::way_back` at the top left of the page, a chevron and that name — *‹ Tracks*,
  *‹ Hypnotize* — and where nothing was left behind it reads the category the page stands under
  and goes there, clearing the scope. It replaced a ghost *Show all* among the actions on the
  right, which cleared the scope rather than returning and was the one control on the page that
  did not look like the way out. Escape is `RootView::step_back` too. The landing is a frame
  behind on purpose — the listing is read on the background executor, so `land_where_it_was_left`
  holds the row until the pane has that many and only then scrolls, the same shape the album
  grid's measured width takes. All three lists land — the albums grid, the tracks and the artists — each through
  a scroll handle of its own, `album_rows` being the grid's, so the row a list was left on is
  read off the list that was showing rather than off `track_rows`. `landing_on` is set only for a
  pane that `Pane::lands_where_it_was_left`, and `set_pane` clears it, so a landing no pane reads
  cannot wait for the next time the artists pane opens and jump it to a stale row.
- **What the window names a queued row by is weighed against the row, not only its id.** The
  engine mints ids from `u64::MAX` down afresh for every restore, every unscanned playlist row and
  every doubled row, so two different files can carry one id a load apart; `LibraryModel::track_of`
  keeps the location and the span beside the name it read and reads again where they disagree.
  `album_title` answers for any album — the scoped one, the listing's, or one read by id into a
  bounded `read_albums` the next listing load throws away — so a search or a page that leaves the
  playing album out does not blank the playback bar, the magnifier's caption or the queue's album
  order. `queue_heading`'s total is measured once per queue revision and library revision into
  `RootView::queue_length` rather than by reading every queued row sixty times a second.
- **A row gesture reads the row the pane drew, not the row the list holds at that index.** A
  playlist narrowed by a search is not reached, so its row menu's *Take out of the playlist* drops
  `Span::one(entry.position)`, the row's place in the list, the way its ✕ already did — `index` is
  its place in the narrowed view. Enter on a reached row inside an album goes through
  `LibraryModel::played_from`, which turns a display row into the track it draws and answers
  nothing for a disc heading or a missing row, the way a click reads `AlbumRow::Held` through
  `played_from_held` — the tracks pane's rows are drawn `Plays::AsTheListingIsDrawn`, where the
  favourites and a suggestion's rows are `Plays::TheseRows`.
- **Every control in this pane is reachable from the keyboard, and it is gpui's own ring.**
  `views/focus.rs` holds one `FocusHandle` per control id and forgets the ones a frame stopped
  drawing, so a control redrawn keeps the caret; the ring itself is `Window::focus_next` and
  `focus_prev` over the tab stops gpui inserts in paint order, so nothing here re-implements an
  order. A control carries `.track_focus`, `.tab_stop(true)` and `key_context(CONTROL_CONTEXT)`, and
  that context is the whole of the key plumbing: `space` and `enter` are bound under it and fire the
  control's *own* `on_action`, escape lets go, and every window binding that used to carry
  `!Search` now carries `!Search && !Control` — so the transport stands down while a control is
  reached exactly as it does for the search field. `tab` and `shift-tab` are bound with no predicate
  at all, because a key that moves focus should move it from inside a text field too. Focus is drawn
  the way hover is: the accent on the border where a control has one, an accent wash where it does
  not.
- **What is in this build, and where it keeps things, is a category rather than a window.**
  `WindowKind::Preferences` and `WindowKind::About` are gone — nothing constructed them, which
  `errors.md` calls a defect. About reports the version, the two faces `fonts.rs` settled on, how
  many sinks the graph advertises and whether a reference started, and it names the settings file
  and the catalog, which reach it as `Places` on `run` beside the `Settings` seam in `Stored`. They
  are readbacks, not settings: `library` has no control because it names the database the pane is
  reading from, and the settings file's path is what `--config` chose.
- **DoP is a claim about hardware, so the pane says so in as many words.** `Command::SetDop`,
  `OutputSettings::dop` and `Setting::Dop` exist because the key did: `EngineConfig::dop` and
  `ConfigKey::Dop` were readable from `config.toml` and reachable from nowhere else — no command, no
  CLI flag, no control. The switch is off by default and its hint says what `audio.md` says: nothing
  in ALSA or SPA advertises DoP, only the DAC's own detector knows, and a DAC that does not decode
  it plays the markers as full-scale white noise.
  `pipeline.rs`'s `no_setting_the_pane_offers_can_produce_a_marked_plan_with_a_stage_in_it` already
  held `dop: true` fixed, so its claim covers the pane now that the pane can set it.
- **The buffer is offered as four fixed depths**, so a `buffer-ms` outside them is reported under
  the choices rather than marked, which is why `choices` takes an `Option`. The resampler group
  prints `SincParams`' four numbers on hover rather than describing a level in adjectives. A device
  named in the file that is not in the graph marks no row, and the group says so rather than leaving
  it to be worked out. The volume is debounced so a held key rewrites `config.toml` once rather than
  once per repeat, and it is the playback bar's control rather than a setting here.
- **Whether a control can be pressed is still the builder's decision.** `kit::Press` and the
  `settings::action` helper are unchanged in kind: a greyed control is built greyed, takes no
  listener and joins no focus ring, because gpui's `hover` carries a `debug_assert!` that a second
  call would trip and a control nothing can press should not be a tab stop either.
- **A pass that rewrites the files is armed before it runs, and the preview is what arms it.**
  *Organising* and *Tagging* are the two, and they are one shape: a *Preview* that runs the pass
  with `Pass::Preview`, an *Apply* that is greyed until a preview has landed and then takes two
  presses — the second under a note saying what the first armed — and a *Stop* drawn only while the
  pass is running. `RootView::moving_the_files` and `writing_the_tags` are the two arming flags and
  `disarm` clears both, so leaving the pane or changing category puts an armed control back. So
  does the pointer leaving the settings body: its `on_hover` lowers every armed press —
  *Reset everything* and the vault's *Keep* with these two — the moment it reads false, which is
  a transition, so a press armed from the keyboard with the pointer already elsewhere stays armed
  until the pointer has been in and out again. The note a finished reset leaves is not an arming
  and stays. Both
  run through `Work`, which is what `LibraryModel::is_busy` reads, so a scan, a forget, an organise
  and a tag write cannot overlap and each greys the others' controls; `Library::retag` takes the
  same `Walk` guard `Library::scan` and `Library::organise` do, so a second caller would be refused
  anyway and the greying is what stops it being asked.
- **The Tagging group draws the plan rather than a count of it.** `listed` is the first
  `WRITES_SHOWN` of `Retagging::writes`, one raised row per file carrying the file name over the
  fields that would be written — `cover art` among them where a picture would go — with *and N more*
  under it where the pass named more, and the summary note after that. Counting alone would have
  been the wrong preview for a pass whose unit is a field: what a listener wants to know before
  arming *Apply* is which files are touched and what would be put in them, and the rows say it. The
  count is still there and still the pass's own — `would write` and the fields and pictures summed
  off the plan before an apply, `RetagStats` after one — so the note cannot disagree with the rows
  above it.
- **What a scan skips can be read again on purpose, one part at a time.** The Library category's
  *Refreshing* group holds *Read every file again*, which is the ordinary scan with `incremental`
  off — `Reading::Everything` beside `Prompted` on `LibraryModel::start_scan` — so every file is
  probed again whatever its size and mtime say, and the names, lengths, sheet cuts, embedded
  covers and search index follow while each row keeps its id, its plays and its favourite; and
  *Look for missing covers*, which is `Library::ask_again_for_covers` and a lookup, whose sweep
  then asks the archive for every album that has a release and no picture. It is greyed with the
  scan's `is_busy`, the covers button with `can_enrich` as well. What MusicBrainz answered is
  Online's *Refresh all*, which the group's note points at rather than repeating.
- **The library's roots are edited through the desktop's file picker.** gpui ships no text input, so
  `App::prompt_for_paths` with `directories: true` is how a folder is named — it is the XDG portal,
  so a machine without one reports through the pane's own notice rather than doing nothing. Adding
  scans just the folders named, which is what registers them; *Rescan* walks the roots already
  registered, and dropping one goes through `Library::remove_root` and forgets every track that came
  from it. The roots are read back with the albums, artists and tracks in one `Loaded` snapshot, so
  nothing else has to know when they change. They live in the library database, not in `config.toml`
  — which is why this is the one setting that does not go through the `Settings` seam.
- **The inspector renders the engine's `StreamDigest`, never a second read of the file.** See
  `audio.md` for how the digest is sampled; `resonate-ui` takes no dependency on `resonate-codec`.
  It opens with the signal path as three `stage` cards — *Source*, the codec in the lossless colour
  over the depth, rate, channels and container; *Processing*, what the output mode did to the
  stream and the gain applied; *Output*, the mode in its colour over what was negotiated and the
  sink — and the detail cards, the bitrate graph and the tags follow under it.
  It follows the *playing* file and nothing else, because inspecting a selected track would mean
  calling `probe_stream` from the window. The live profile restarts on every rebind — a seek, a sink
  switch, a renegotiation — because a `ProfileBuilder`'s windows are sequential and cannot be
  resumed mid-span, so the graph covers the decoded span since the last rebind, which is what
  `StreamDigest::profiled_from` names. The digest is rebuilt once per closed window, so once per
  second of decoded audio, and the decoder runs ahead of the sink by the ring's depth, so the graph
  leads what is being heard. It is drawn as a line rather than as bars: `canvas` and `PathBuilder`
  stroke a polyline over a filled area washed from the accent down to nothing, because what a
  listener reads off a bitrate graph is the rise and the fall between windows rather than the height
  of any one of them, and a `div` per column cost a layout node per point. The line is condensed at
  the digest's rate rather than at the frame rate: `PlayerModel::condensed` holds the series the
  columns were reduced to and throws it away when the poll swaps the digest for a different `Arc`,
  because `StreamProfile::series` runs to `MAX_WINDOWS` — a whole day of windows — and reducing it
  to `GRAPH_COLUMNS` points sixty times a second is a walk and an allocation per frame for a picture
  that changes once a second. A profile of one window is drawn as a level line, because one window
  is one reading held across the whole span rather than nothing to draw.
- **The visualiser draws what the tap says is being heard, and its arithmetic has no gpui in it.**
  `Pane::Visualiser` sits under `Section::Playing` beside the lyrics and the inspector — the same
  six edits the three panes before it were — and follows the playing track and nothing else: with
  nothing playing it is `kit::empty` under `Icon::Visualiser`, a silhouette of bars under their
  held peaks, and a DoP stream is `kit::empty` saying it carries markers rather than samples. The
  heading is the lyrics pane's — the title and the artist through `opens` — with a `kit::figure`
  reading back what is analysed, *4096-point · 48 kHz*, the hint, and a `kit::segmented` choosing
  *Spectrum* or *Scope*, a choice that lives on the entity for the run. `spectrum.rs` is the
  arithmetic, tested the way `curve.rs` is: a radix-2 real transform by hand — a half-length complex
  one over the bit-reversed even and odd samples, split back into the real spectrum — under a
  periodic Hann window, scaled so a full-scale sine on a bin reads 0 dBFS; sixth-octave bands
  across the equaliser's own `RESPONSE_FROM_HZ` to `RESPONSE_TO_HZ`, placed by `across_at` and
  labelled by `marked_frequencies`, so the two plots read 100, 1k and 10k at the same places; a
  band narrower than a bin read at its centre between the two bins either side, and one past the
  stream's Nyquist left on the floor. Each band is its loudest bin tilted up
  `TILT_DB_PER_OCTAVE` about 1 kHz, so pink noise stands level and a mastered record does not
  slope away into the treble, drawn between `FLOOR_DB` and 0 over faint lines every 12 dB — which
  is why the levels carry no figures, a tilted reading being dBFS only at the pivot. A bar rises at
  once, falls at `FALLS_DB_PER_SECOND`, and its peak is held `PEAK_HELD_FOR` before it follows,
  all in wall-clock time so a frame that comes late falls further rather than slower. The window is
  sized by the rate, about 85 ms: 4 096 points at 44.1 and 48 kHz, up to 32 768 at 384. The scope
  is 20 ms of left over right, starting on the first rising zero crossing of their mid in the first
  span of a window twice that long, so a steady tone stands still rather than crawling.
- **The pane repaints on the player's poll and never on the display's clock.** gpui marks every
  ancestor of a notified view dirty — `Window::mark_view_dirty` walks the view path — so a frame
  the pane asks for re-runs `RootView::render` however much the pane is an entity of its own, and
  `request_animation_frame` asks at the display's rate: on this machine's 240 Hz output the first
  cut of the pane took the window from about 6.5 % of a core to 32.5 %. So `Visualiser` observes
  `PlayerModel` and draws when the 16 ms poll notifies, which it does on every engine publish while
  anything plays — the frames the window was already drawing — and asks for none of its own; only
  bars still falling once nothing is tapped ask again, on a timer at the poll's interval, and a
  paused transport holds the paused moment and asks for nothing at all. Being an entity is what
  keeps its state — the transform's tables, the bars, the three sample buffers — out of
  `RootView`, and what lets it keep drawing if the root's observer is ever narrowed. Measured with
  a 24-bit 192 kHz FLAC resampled to a 48 kHz device over an empty scratch catalog, the window takes
  7.6 to 7.7 % of a core with the pane in front and 6.4 to 6.8 % with the tracks pane, the engine
  thread 1.1 to 1.2 % either way: the pane's own cost is its transform and its sixty-odd quads, and
  the whole-window render under it is the one `docs/TODO.md` already records. `RootView::render`
  calls `PlayerModel::listen_in` with whether the pane is in front, so the engine taps nothing
  while it is not.
- **The analysis pane draws the whole of the playing track and says whether it is what it claims.**
  `Pane::Analysis` is the fourth pane under `Section::Playing`, the same six edits the three before
  it were, with `Icon::Analysis` a waveform over its axis. Its heading is the visualiser's — the
  title and the artist through `opens` — reading back the codec, the declared depth and the rate,
  or how far the decode has got while it runs. Under it: the verdict in its colour over one
  sentence per finding, each `Finding::told`; the waveform, a lane per channel drawn as a min-max
  polygon washed in the accent with the RMS solid inside it, the part heard shaded and a playhead a
  press seeks from; the spectrogram, the painted PNG stretched to the card with its frequencies as
  backed labels and the cutoff as a line in the verdict's colour; the average spectrum as a washed
  polyline over faint lines every 20 dB; the levels and the source as `fields` cards dealt by
  `beside`, which the inspector lends; and the recognition card, led to the top where the audio is
  not the song the file names. `analysis_plot.rs` is the arithmetic, with no gpui in it. The
  Online category's *Recognition* group is the `acoustid-key` field, written through the
  `Settings` seam the way *Contact* is and read from the next start. `analysis.md` has the model.
- **Listen is a sheet over whatever pane is in front, not a pane of its own.** It names a song that
  need not be in the library, so it belongs to no section: `ctrl-l`, bound in `answering_anywhere`,
  and the ear in the header — which stops its press like every other header control — both open it
  through `RootView::open_the_listener`, which starts a recording at once, and escape or a press
  outside the card closes it and stops one in flight. Stopping is the same in both stages: it
  drops the task that would ask about the clip and puts the sheet back at `Idle`, so a song named
  after Stop or after the sheet closed is neither told to the desktop nor drawn. `listening.rs` is the model and `views/listen.rs`
  the drawing. `ListenModel` holds the `Listens` the binary handed in and walks one `Stage` —
  `Recording` with the `Hearing` the bar is drawn from, `Asking`, then `Found`, `Unknown`,
  `Silent`, `Unreached` or `NoService` where no recogniser is registered — and the recording and
  the asking each run on the background executor, so the frame never waits on PipeWire or a
  network. A found song is a card: the cover at `theme::listen_cover()`, the title, the artist, the
  album and year, which service named it, and *Listen again*, *Open* where the service gave a link
  and *Find it*, which is `search_instead` and `choose_pane(Pane::Tracks)` over the title and the
  artist, so a held track is one press away and a song not held lands on the search that lists
  what the catalog and MusicBrainz know of it. The last `HEARD_KEPT` songs stay listed under it for
  the run. *Desktop* and *Microphone* are a `kit::segmented` over the chips of the microphones
  PipeWire offers, listed each time the sheet opens, and a choice is written back as `listen-from`
  through the `Settings` seam; the Online category's *Listening* group writes the same key and
  `listen-for`, and its *Recognition* group carries the AudD token beside the AcoustID key, each a
  `Field` read at the next start.
- **Play next and Add to queue sit beside every play gesture, and a + wherever rows reach a
  playlist.** The tracks heading and each track row, the playlists pane's index row, an opened
  playlist's heading and each of its rows all carry the pair, and on a playlist row they take the
  whole reach the row is in. The + sits on a track row, a queue row, an opened playlist's row and
  the index row that stands for a whole playlist, and every one of them opens the one picker. The
  albums and artists panes carry neither, because a row there scopes the tracks pane rather than
  playing, and the queue pane carries neither because its rows are already in it.
- **Every major listing is put in order, from a chip row and from its header.** The tracks, albums
  and artists panes each hold an order and a reading on `LibraryModel::sorting`, kept for the run
  like the settings category and the playlists order. Two controls write it: a *Sort* action
  opening the ORDER and READS chips, and the column header, which is pressable — a press names the
  order, a second press on the one in force turns it round, and the column in force wears the
  accent and a chevron. `views/sorting.rs` is where both live: `Ordering` is the trait the five
  order enums implement and `sorting::order_row` is the one chip-row builder, which is what the
  three near-identical builders in `views/playlists.rs` collapsed into. `listing::columns` grew a
  `Sorted` descriptor — which columns a listing offers, which is in force, and a `fn` pointer for
  the press — so the tracks pane, the queue and an opened playlist still draw one header and
  cannot disagree about a width, while offering different vocabularies. A right press on a header
  offers every order it has.
- **The queue is put in order as a gesture, not as a standing order.** Its heading's *Sort* sends
  one `Command::Order`: the window builds a permutation over the rows it draws — scanned or read
  through `Player::media` — and the engine rewrites the play order in one pass with the cursor
  re-seated onto wherever the playing row went, so a sort mid-track reopens no stream. A queue row
  is still dragged and reached exactly as a playlist row is, and what comes next is still
  shuffle's and the transport's to say. The queue's heading carries its count, its total length and the row in
  play, and *Clear* sends one `Command::Remove` over the whole span. The reach lives as long as the run, escape clears it, and one left past the
  end of a shorter list is pulled back to the last row rather than dropped.
- **A heading wraps rather than grinding its name down, and `kit::actions` is the whole of how.**
  Every heading's controls sit in one, so the longest of them — an opened playlist's Rename, Export,
  Copy, Drop shown under a search, Sort, Tidy, Fold doubles, Play next, Add to queue and Play —
  flows onto a second line as the window narrows rather than clipping Play off the edge. The
  actions take at most 64 % of the row and the name keeps `theme::heading_name()` of room whatever is
  beside it, which is what makes the controls wrap rather than the name grind down to a letter as
  the thirteenth control arrives. An album's and an artist's pages are out of this entirely: their
  actions sit under the name, so nothing is beside it.
- **Undo and Redo are drawn in the playlists headings and nowhere else.** `RootView::undo_edit` and
  `redo_edit` are the two, the Redo drawn only while a step is there to put back, and each says how
  many steps stand behind the one it offers — `Undoable::behind`, the only number either stack
  publishes about itself. Rows put in a playlist from the tracks or queue pane have no control to
  press, so the notice names `ctrl-z` beside where the rows landed; the notice a press either way
  leaves says what was put back or done again rather than what the playlist now holds.
- **A narrowed playlist says what a gesture reaches rather than leaving it to be found out.** The
  heading's `NARROWED_HINT` says that Play, Play next, Add to queue, Copy and Drop shown take the
  rows shown while Rename, Sort, Tidy and Export take the playlist whole. It is behind the pointer
  like every other hint, and a search that matched nothing draws no mark to hover. The sidebar's
  count is the narrowed one, so standing inside a playlist whose name the search misses reads as 0.
- **A pane hands its rows out by reference count rather than cloning them per frame.** An opened
  playlist's rows are an `Arc<[PlaylistEntry]>` the model shares with the pane, its heading, the
  list's closures and every visible row, and the reach is read off it once a frame as a `Reaching` —
  the rows and their locations — which a row inside the reach takes by reference count too. `Held`
  carries an `Arc<[MediaLocation]>` for the same reason, so the picker's rows are copied only where
  the press hands them to the library. The browse panes read the same way: `LibraryModel` holds the
  albums, artists and tracks as `Arc<[_]>` and `albums()`, `artists()` and `tracks()` hand back a
  pointer, so the tracks pane no longer deep-clones two thousand `Track`s — each a `PathBuf` and two
  `String`s — on every one of sixty frames a second. The queue pane reads the same way, and a whole
  list is walked only where a press asks for one: the heading's *Copy*, the index row's +, the
  queue's *Save as a playlist* and the tracks heading's *Add to playlist*, *Play next*, *Add to
  queue* and *Play* each do it inside the listener rather than on every frame that may never carry
  the press — the fresher read as well as the cheaper one.
- **The playing row is indexed, not searched for, and it is resolved once a pane.**
  `PlayerState::queue_position` already names the row, so `RootView::queued_row` reads it out of the
  published queue by index and weighs the id it finds against `state.current` rather than walking
  the queue for a match. One `Playing` carries everything the playback bar draws — the title, the
  artist, the album, the codec and a `Cover` of the album id and the file the picture comes from —
  so the panel, the cover cell and the signal path are one resolution rather than three walks and
  two `Track` clones.
- **A playlist edit re-reads the playlists and nothing else, and so do a scan and a counted play.**
  `LibraryModel::reload_playlists` takes the `ThePlaylists` read above; `reload` re-reads the opened
  playlist beside the listing, which a scan runs every twenty polls and again when it ends and a
  counted play runs once, so `plays:` and `played:` queries move under the window as `added:` does.
  Inside that read the listing is whole, so a rename re-reads the entries beside it: the counts and
  the total length are one grouped pass over the entries rather than two subqueries per row, and the
  opened playlist is read once more beside them, because a narrowed index would lose the row the
  heading draws its name, count and kept order from.
- **A second process's edit is seen once its writes settle.** `watch_for_writes_elsewhere` reads
  `Library::written_elsewhere` — the writer connection's `PRAGMA data_version`, typed as a
  `WrittenElsewhere` that is only ever compared — on a background thread every `WATCHED_EVERY`,
  and reloads once the stamp has moved since the last reload *and* read the same on the tick
  before, so a playlist or a favourite `resonate mcp` changed, or a `resonate scan` beside the
  window, is drawn a few seconds after it lands, and a scan committing batch after batch costs
  one reload when it pauses rather than one per tick. This process's own writes never move the
  stamp, so the window's own edits are still the reloads they always were. A busy writer answers
  nothing and the tick is passed over.
- **`Raise` does nothing and `Quit` closes the window.** `App::activate` is all gpui offers and a
  Wayland compositor is free to refuse a window carrying no activation token, so `Host::can_raise`
  answers false under the GPUI front end. `Quit` sends on the same channel `resonate play` uses, and
  the gpui foreground drains it on the frame timer.
