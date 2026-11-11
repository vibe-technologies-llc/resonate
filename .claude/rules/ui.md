---
paths:
  - "crates/resonate-ui/**/*.rs"
---

# The window

`resonate-ui` is GPUI: on Wayland, and the same window on Windows (`windows.md`) and macOS
(`macos.md`). Depends on engine, library, lyrics, `resonate-listen`; not on
`resonate-codec` or image decoding (`image` only for the frame gpui's `RenderImage` is built of). A
`Reference` arrives only as the trait object the binary hands `run` in `Lookups`; the online crate
is never named.

## Chrome and input

- **Title names what plays.** `RootView::name_the_window` sets *Echoes — Pink Floyd · Resonate*
  (artist omitted if blank), else *Resonate*; only when the text moved
  (`the_window_is_titled_by_what_plays_and_by_the_app_where_nothing_does`).
- **The window is its own titlebar.** `WindowDecorations::Client`; the header draws
  minimise/maximise/close, drags the window, opens the compositor's window menu; under
  `Decorations::Server` neither controls nor drag handlers render, except where the header stands
  in the system's title bar (`chrome::THE_HEADER_STANDS_IN_THE_TITLE_BAR`, which is
  `TitlebarOptions::appears_transparent`): on Windows the caption is the window's own and the system
  presses it (`windows.md`); on macOS the system draws its traffic lights inside the header, which
  only moves the window. Who draws the controls and what moves the window are two questions:
  `chrome::the_window_controls_are_ours` (never on macOS) and `chrome::the_header_moves_the_window`.
  - **On macOS the traffic lights stand in the header.** `chrome::traffic_lights_placed` is
    `TitlebarOptions::traffic_light_position`: `TRAFFIC_LIGHTS_FROM_THE_EDGE` (16) in and mid-height
    of `theme::header_height()` (19 down at the middling text size, a light being
    `A_TRAFFIC_LIGHT_IS_TALL`, 14), since gpui makes the title bar a light's height plus twice that
    drop. The wordmark is inset by `chrome::room_for_the_traffic_lights` (70: the lights' 54 and
    the 16 before them) inside its own column, so the search field stays centred, and by nothing
    while the window fills the screen, where the system hides the lights. A press on the bar is
    noted and the first move after it calls `Window::start_window_move`; a double press is
    `Window::titlebar_double_click`, which does what the system's setting says; a right press opens
    nothing. The place is set as the window opens, so a text size changed while it is open leaves
    the lights where they stood until the next start. `WindowDecorations::Client` is ignored there
    (`Decorations::Server`), so `chrome::frame` draws no border, rounding or shadow of its own
    (`the_traffic_lights_stand_mid_height_of_the_header_whatever_its_size`,
    `the_header_keeps_room_for_the_traffic_lights_only_while_the_system_draws_them`). Never seen on
    a Mac (`macos.md`).
  - Header: `theme::header_height()`, three columns; wordmark and controls each take sidebar width
    less padding, so the search field (at most `theme::search_width()`) centres on the window.
    Control = `theme::WINDOW_CONTROL` (31, odd so the mark centres on a whole pixel) square.
    *Resonate* drops `WORDMARK_CAPITALS_CENTRED_BY` to centre capitals on the accent square. The square
    is `theme::wordmark_mark()` and grows with the text size, as the chosen tab's accent bar
    (`theme::chosen_mark()`, centred in its `sidebar_row`) does: fixed, both read as left behind
    beside text drawn twice as large.
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
- **Closing removes the window; quitting follows, never the reverse.** Close control, the quit key
  (`ctrl-q`, ⌘Q on macOS), the menu's *Close* and *Quit* and the bus's `Quit` call
  `Window::remove_window`; `app.rs` quits once the last window closed (`App::quit` under a live
  window logs *window not found*).
- **What is done on leaving is done once, from gpui's own quit.** `run` takes a `Leaving`, the
  binary's teardown (the tools, the presence, sync, the party room, shared playlists, submitting,
  the zones), and runs it after its own two `wind_down`s from a `cx.on_app_quit` observer, or
  after `Application::run` returns where that came first (`LeftOnce`): on macOS `run` never
  returns, the system ending the process inside `terminate:`, so anything written after it was
  never run there (`what_is_done_on_leaving_is_done_once_whichever_way_the_window_leaves`).
- **What the system opens is handed to the window as a second launch hands it.**
  `Application::on_open_urls` (a file opened from Finder, a `resonate://` link) sends each batch
  down `Bus::opened`; the binary's `resonate-opened` thread waits for the window to answer
  (`WHAT_IS_OPENED_WAITS_FOR_THE_WINDOW`, 15 s, for a cold start) and queues it through
  `queued_in`, the half of `handed_to` after the tuning, so an invitation link is taken as an
  argument is. Only macOS sends any.
- **The menu bar is data, laid out on every system and kept by macOS alone.** `app_menu::lay_out`
  calls `cx.set_menus` after the keys are bound (gpui reads each item's key off the keymap, the
  earliest binding of its action): *Resonate* (About, Settings…, Services, Hide, Hide others, Show
  all, Quit), *Edit* (Cut, Copy, Paste, Select all, as `OsAction`s over the field's actions, so
  the system's own panels take ⌘V), *Playback* (Play or pause, Stop, Next, Previous, Shuffle,
  Repeat, Louder, Quieter, Mute), *View* (Queue, Search, Listen, Full screen) and *Window*
  (Minimise, Zoom, Close), its words `words/en/app_menu.ftl`. `Hide`, `HideOthers` and `ShowAll`
  are answered by the app (`cx.on_action`), `About` (the About category), `Minimise`, `Zoom`,
  `CloseTheWindow` and `ToggleFullScreen` by `RootView` beside `Quit`. **A menu item fires
  whatever holds the caret**, gpui's menu dispatch reading no key context, so: *Undo* and *Redo*
  are left out (they would walk a library edit back while a field is typed in); *Play or pause* is
  `TogglePlayPause`, which a Mac binds to no key, never `PlayPauseUnlessTyping`'s `space`; and no
  action in the menu is bound to a key without Command or Control
  (`a_key_the_menu_shows_beside_an_item_is_never_one_a_listener_types`). The *Window* menu is
  found by gpui under the literal name `Window`
  (`the_menu_a_mac_lists_its_windows_under_is_named_as_gpui_looks_for_it`), so a language that
  renames it loses the system's window list.
- **A group that says nothing on a system is not offered there.** `Group::is_offered` leaves it
  out of the pane and of the settings search: *Graph rate* and *Bluetooth* are PipeWire's alone
  (hidden on Windows and macOS); *Notifications*, *Metered connections* and *Window buttons* are
  hidden on macOS, where nothing notifies, no meter is read and the system draws the buttons
  (`a_group_that_says_nothing_on_a_system_is_not_offered_there`). The sound system a sentence
  names is `system::sound_system!`: *PipeWire*, *Windows* or *Core Audio*.
- **The sidebar scrolls its middle and keeps its foot.** `RootView::sidebar` holds the tabs in an
  `overflow_y_scroll` region tracked by `sidebar_scroll`, with a `Scrollbars` bar over its edge; the
  download and enrichment status and *Settings* stand below it, never scrolled, so a window squeezed
  vertically scrolls the tabs and leaves the foot above the playback bar. The two status rows are a
  pane row's height and inset with a `theme::pane_icon()` mark, so their marks and words line up
  with the tabs above and *Settings* below; a pinned playlist's row is `theme::pinned_row()`.
- **A long menu nests what a row rarely needs, one level deep.** `Menu::nests(icon, label, fill)`
  hangs a panel off a row with a chevron (`Takes::Opens`, nothing when `fill` offers nothing): every
  menu is the same groups in the same order, a line between each: what plays (*Play*, *Play
  next*, *Add to queue*), what keeps (*Add to playlist…*, the favourite), where to go (*Go to* with
  artist and album, *Other copies*, *Find another version*; an album's or artist's own page, then
  *Show the artist*), what leaves the window (*Share*, *Copy* with title, artist, album and file
  path, *Show in the file manager*), *Identify* (nesting *Enrich this song*, the ISRC and *Place on
  a release…*, wherever the reference is reachable), and last what takes something away (*Take out
  of the queue*, *Remove* with hide, forget a delivery and delete; a playlist's *Discard*). The
  shared pieces (`Menu::holds`, `favours`, `reaches`, `shares`, `offers_the_file`,
  `offers_the_files`) draw no line of their own; the caller puts `apart()` before each group. **Another version of a song is looked up from *Find another version***, which nests *Find a higher quality version* and the ratings the track is not already (*Look for a Clean version* for an explicit track, *Look for an Explicit version* for a clean one, both for an unmarked one: `better::Sought::offered_for`): `views/better.rs`. Held copies are offered best first: for quality `alternatives_of` where `Quality` (lossless, significant bits, rate) is no lower on any count and higher on one, for a rating the catalog's tracks named alike (`LibraryModel::tracks_named`, `HELD_WEIGHED_AT_MOST`) that `TheSong::held_as` the rating. Every service that plays in place (`Serving::opened`, so nothing is asked with Online off) is searched on the background executor for the bare title and artist, a song kept only where it is the same song (folded bare title, artist, length within `LENGTHS_AGREE_WITHIN`, 3 s) and, for quality, its suffix reads as a lossless codec (`resonate_library::codec_named`), for a rating it carries it: Explicit must be marked, and Clean may be unmarked only beside an explicit track (`a_clean_version_of_an_explicit_song_may_be_unmarked_and_an_explicit_one_must_be_marked`). **Every lookup says what it is doing and what came of it**: *Looking for … of {title}* as it asks, *Found N versions of {title}* with the menu at the press, a `Trouble` naming every service that could not be asked, and where nothing was found the reason (`better::nothing_said`: already lossless, no service set up to ask, or none on the services named; `nothing_found_says_why_and_where_it_was_looked_for`). **A version picked is the song's from then on**: the menu entry plays it and keeps it in `versions_chosen` (`Library::choose_version`, `library.md`), told as *{title} plays this version from now on*; `RootView::send` passes every `Command::Load` and `Insert` through `with_the_versions_chosen`, so a row of that song wherever it is played (the tracks pane, a playlist, a queue gesture) is queued as the held copy or the served location instead, a served one under a minted id (`better::Chosen::in_place_of`, `a_song_with_a_version_chosen_is_queued_as_that_version_wherever_it_is_played`, `a_playlist_row_plays_the_version_chosen_for_its_song_until_its_own_copy_is_asked_for`). *Play your own copy again* stands in the nest while a version is chosen and forgets it. An opened playlist's row menu offers *Find another version* too. A pointer opens the panel
  by resting on its row and a press too; it stands beside the row on whichever side has room
  (`beside_its_row`, read from the row drawn a frame behind), and resting on another row closes it only
  after `STAYS_OPEN_FOR_THE_POINTER_CROSSING` (250 ms), which reaching the panel calls off, so a
  diagonal to it does not lose it. `right` or `enter` steps into a panel from its reached row, `up`
  and `down` walk within it and `left` or escape steps back onto that row, a second escape closing the
  menu. `Menu::labels` lists a nest's rows in its place and `Menu::offering` says how to reach a label
  (`a_menu_beneath_is_walked_by_the_keys_and_stepping_out_lands_on_the_row_it_hangs_from`).
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
  - **An account given is asked at once.** Every field of the Subsonic, Jellyfin and TIDAL groups writes the
    global `Online` as it writes the file (`Sourcing::providers`). `the_sources_moved` calls
    `LibraryModel::sources_moved`: every *No provider is set up* download returns to *Queued* and
    every unheld want is asked, as *Poll now* does; turning Online on does the same
    (`a_song_asked_for_before_any_provider_was_set_up_is_fetched_once_one_is`).
  - A server address must begin `http://` or `https://` with a host (`reads_as_a_server`); else a
    toast, nothing stored.
- **Jellyfin is signed in to with Quick Connect from its group** (`views/settings/jellyfin.rs`): the
  `SignsInToJellyfin` seam (`Lookups::signs_in_to_jellyfin`, the binary's `QuickConnects`;
  `JellyfinBegun`, `JellyfinSignIn`, `JellyfinSession` with its token withheld) behind *Sign in with
  Quick Connect* (`jellyfin-quick-connect`) beside an address not under `/emby`; the code
  (`jellyfin-quick-connect-code`) waits beside *Cancel* (`jellyfin-quick-connect-cancel`) and the
  sentence to approve it. `approved` is asked by the code shown, and `QuickConnects` keeps each
  waiting approval under its code, so a begin left over from an address since changed never stands in
  for the one shown. Signed in the user field holds the server's name for the user, the
  password field empties and the three session settings are written. Quick Connect off is a note that
  keeps the password; a password typed, the address changed or the account forgotten lets the session
  go (`let_go_of_the_jellyfin_session`;
  `a_jellyfin_code_approved_elsewhere_signs_in_and_empties_the_password`,
  `a_password_typed_after_a_quick_connect_lets_the_kept_session_go`).
- **A Spotify account is signed in to, followed and let go from its group, and offered only with a
  client ID.** `views/settings/spotify.rs` holds one `Field` (`spotify-client-id`, an app the listener
  registered). `SignsInToSpotify` is the seam, `Lookups::spotify` the binary's
  (`online::signs_in_to_spotify`, `None` without `online`), so the window names no online type:
  *Sign in to Spotify* is greyed without a client and Online and asks nothing then; pressed, it binds
  the redirect and opens the address (`cx.open_url`), waiting beside *Open the page again* and *Stop*
  on the background executor; the token lands in the global and the file, never in a field. Signed
  in, *Follow the account* and *Move to Resonate once* run `Library::follow_profile` on
  `ProfileLink::SpotifyAccount` (`followed_the_account`, a following toasted as a pasted profile is),
  a refusal under `LookupOp::SpotifyAccount` saying to sign in again; `refreshed_where_due` reads a
  followed account every `KEPT_IN_STEP_EVERY` as any profile, and one signed out names nothing and
  costs nothing. *Sign out of Spotify* clears the token and lets the account's playlists go with their
  rows. `resonate spotify` reaches a running window (`Followed::Spotify`, `Reloadable::Spotify`)
  (`a_spotify_account_is_signed_in_to_followed_and_let_go_from_its_group_and_offered_only_with_a_client`,
  `a_spotify_sign_in_made_at_the_terminal_reaches_the_window`).
- **Last.fm is signed in to on its own page; no password is typed in the window.**
  `views/settings/lastfm.rs` holds two `Field`s (`LastfmAccount`): API key and secret, kept as
  `lastfm-key`/`lastfm-secret`. *Sign in to Last.fm* asks `Scrobblers::lastfm_approval` on the
  background executor, opens the page it answers (`cx.open_url`) and waits on the approval beside
  *Open the page again* (`lastfm-open-again`) and *Stop* (`lastfm-stop`); a session is written as
  `lastfm-session`, an end without the allowing is a note that nothing was kept, a refusal says to
  check the key and secret, anything else that Last.fm was not reached
  (`last_fm_is_signed_in_to_by_allowing_it_on_its_own_page_and_no_password_is_asked`).
- **A secret is drawn as marks.** `Field::masked` (Subsonic and Jellyfin password, TIDAL secret and token,
  AcoustID/AudD/ListenBrainz keys, Last.fm key and secret) shapes one `•` per letter; `Shown`
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
  `xf86audioprev`; `XF86Back`/`Forward` map to `back`/`forward`. All seven are
  `answering_to_the_keys_a_keyboard_carries`; `app.rs`'s test pins the names against
  `Keystroke::parse` (an unreadable name binds to nothing, silently). **None is bound on macOS**:
  gpui reads no system-defined event there, so the keys never reach a window (Now Playing answers
  them, `macos.md`), and a name bound would be shown beside a menu item as its key
  (`a_mac_binds_none_of_the_keys_only_another_keyboard_carries`).
- **Typing at a list no search narrows jumps through it.** The search narrows tracks, albums, artists,
  playlists-index and opened-playlist panes; the queue narrows on nothing, so `RootView::typed`
  routes a keystroke there to `TypeAhead` first. `jumped` prefers a row *starting with* the letters,
  wraps; `RootView::jumping` is the one place saying which panes opt in. Pill says *Jump to* or
  *No match* in words, no id, no listeners; held `HELD_FOR` (1100 ms). `typing.rs` holds the
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
    context), `answering_on_a_band` (`BAND_CONTEXT`) are four lists; `bindings(system)` concatenates
    them with `field::bindings(system)`, behind the keys a keyboard carries or, on macOS,
    `answering_as_a_mac_expects` (below). A new key goes in one;
    `every_key_not_named_to_answer_anywhere_is_dead_while_a_caret_is_held` walks the second on
    every system, and no chord stands twice under one predicate
    (`no_chord_answers_twice_where_the_same_things_hold_the_caret_on_any_system`).
  - Escape blurs and clears, closing the naming row before clearing a search; a press outside blurs
    via `on_mouse_down_out`. `ctrl-f` takes focus deliberately (no predicate) and selects what is
    there.
- **Escape away from the field is a chain, and a notice is in it.** `LeaveSearch` binds under
  `SEARCH_CONTEXT` alone; `RootView::typed` hears escape elsewhere (`RootView::escaped`). A ringed
  control's escape is `LeaveControl`, which fires before `typed`, so it runs the over-the-pane part
  (`close_what_stands_over_the_pane`) first and lets the control go only where nothing stands there:
  a sheet's focus lands on its safe control, and escape there still closes the sheet. Order, over-the-pane before
  beside-it: delete sheet (`keep_the_track`), Listen sheet, magnified cover, playlist picker
  (`stop_naming`), shown record, Wrapped, sharing, Sort pop-up, open menu, downloads panel, standing toast (`toast::dismiss`), live
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
  record card, Listen sheet, Sort pop-up or delete sheet is `something_stands_over_the_pane`; meanwhile reach,
  page, widen, move, `enter`, `delete`, undo, redo and typed letters answer nothing. The check sits in
  each gesture, not the bindings; `reach_row` and `play_reached_rows` step and press the menu first.
  Escape is the one key still heard (`views/root.rs`'s driven tests hold each sheet).
- **A slider is grabbed on the press and followed from a window-wide surface, not the rail.**
  `views/slider.rs`: the press starts a `Grab`; `drag_surface` (an absolutely positioned *child* of the
  app, occluding while held) carries move/release listeners. It must be a child: `chrome::frame` adds
  its own `stop_propagation` move listener to the app element and one element's listeners run in
  reverse order. gpui reports motion only inside the window, so a drag past the edge freezes at the
  last inside value; `on_mouse_up_out` still hears the release.
  - Volume applies on every move (`VOLUME_SETTLE` debounces `config.toml` writes, kept as the zone shown at the change: `volume_unkept`, flushed by `show_zone` and as the window leaves, so a switch neither files it under the other zone nor loses it); holding shift on the press
    or the drag snaps it to a `VOLUME_STEP` (5 %, `snapped_by`), the seek rail ignoring the key. A seek only previews
    and commits one `Command::Seek` on release, and only if the press's track still plays
    (`a_seek_rail_released_after_the_track_changed_seeks_nothing`); `PlayerModel::seek` holds the
    requested position until its `Outcome` answers (a refused seek restores the published position
    and raises the command toast); only `shown_position` reads the target, `state` stays what the
    engine published.
  - Wheel over the volume cluster = a `VOLUME_A_NOTCH` (5 %; touchpad pixels in `PIXELS_A_NOTCH`) while
    `ResonateApp::scroll_volume` says so; gpui hands one click of a wheel over as three lines
    (`LINES_A_WHEEL_NOTCH`, its `SCROLL_LINES`), so a line read as a notch moved 15 % a click;
    `RootView::volume_aimed` is what a run of notches or keys adds to (reading the published volume
    back lost all but one per poll). **The volume rail is an even number of pixels wide**
    (`halves_on_whole_pixels`): at a text size scaling it to an odd width, 50 % filled one side a
    pixel further than the other.
  - **A press on the speaker, or `ctrl-m`, mutes; mute is not a volume.** `toggle_mute` sends
    `Volume::MUTE` and keeps the heard level in `muted_from`, storing nothing. `muted_at` reads the
    pair only while the engine publishes mute, so a volume set over the bus ends the mute. Where the
    device has the slider (`OutputStatus::device_turned`) the press is `Command::SetDeviceMute`
    (`audio.md`).
  - Release is taken from three places (inside the window, `on_mouse_up_out`, a move reporting no
    button held). The equaliser's handles ride the same surface: a `HeldBand` beside the `Grab`
    (`eq.md`).
- **The window shows one zone at a time; the switcher in the header chooses it.** `ResonateApp::zones`
  is the engine's `Zones`; `RootView::zones` holds a `ZoneShown` (`PlayerModel` entity, `Listening`,
  `Keeping`, last `KeptZone`, observer) per zone and `RootView::player` is the selected one's, so the
  queue, transport, now-playing, output picker and visualiser shown are that zone's and nothing else
  changes. `views/zones.rs`'s `Switcher` is the windowless model (selection, add, remove falling back to
  the zone before, rename, the label and what a screen reader hears), tested there. The control sits
  before the window controls: a speaker mark alone with one zone, the selected zone's name beside it
  once there are several; a press or `enter` opens a `Menu` listing the zones (*Check* on the shown
  one), with several a *Move playback to …* entry per other zone (`Switcher::elsewhere`,
  `moving_to`), *Add a zone*, *Rename this zone…* and, off the first, *Remove this zone*. **Moving
  playback** (`RootView::move_playback_to`) runs `Zones::hand_over` and its `wait` on the
  background executor and shows the incoming zone at once; landed, a toast says *Playing in … now*
  (`moved_to`); refused or unheard, the window goes back to the zone it came from and
  `zones::refusal` names why (`HandoverUnheard`, `NothingToHandOver`, `DeviceHeld`). Switching never
  touches playback (`show_zone` keeps a volume change waiting, moves `listen_in` to the shown zone and clears the volume aim, mute
  memory, seek grab, queue selection (`reach`, `landed_by_a_jump`), queue names and take-out offer, which belong to one queue). Adding and removing
  run on the background executor (a player opens its own PipeWire client). Renaming swaps the control
  for a `Field` (`zone_name`, in `text_fields`): `enter` names it, escape or a press outside leaves it.
  `RootView::send` goes through `Zones::send`, and a refusal is a toast naming the zone in the way
  (`zones::refusal`); `store`/`forget` drop the `sink` and `volume` keys for any zone but the first
  (`SettingKey::is_the_zones_own`), its device and volume being kept with the zone. The output
  picker routes through `route_to` and marks a device another zone holds *Held bit-perfect by …*.
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
- **Seventeen themes are dark and two are light; contrast tests hold each readable.** Nineteen
  (`Theme::ALL`), the two light ones last: dark are Resonate's own warm near-black, Midnight,
  Graphite, AMOLED, Plum, Rosé Pine, Rosé Pine Moon, Catppuccin Mocha, Macchiato, Frappé, Nord,
  Gruvbox Dark, Tokyo Night, Dracula, One Dark, Solarized Dark and High Contrast (pure black
  ground, white text, every bar below at 7:1, drawn edges at 4.5:1; `high-contrast`); light are
  Resonate Light (`resonate-light`, warm paper ground, the same mauve native accent) and High
  Contrast Light (`high-contrast-light`, pure white ground, black text, the same 7:1 and 4.5:1
  bars). Each carries the same seven accents (`Accent::ALL`: Mauve, Blue, Teal, Green, Amber,
  Peach, Red) under its own hex; on a light ground they are drawn deep (lightness .26, and .16 in
  High Contrast Light) because an accent is also text and must reach 4.5:1 (7:1) on the panes.
  - **A theme has a `Ground` and a counterpart on the other.** `Theme::ground()`, `counterpart()`
    (High Contrast and High Contrast Light are each other's, Resonate Light's is Resonate, every
    other dark theme's is Resonate Light) and `on(ground)` (the theme itself where it already
    stands on that ground) live in core, tested there.
  - **On a light ground everything that assumed a dark one reads through the ground.** A shadow is
    `theme::shadow(strength)` (black, its alpha 40 per cent of the dark figure on a light ground;
    no view builds an `hsla` of its own), a scrim is the palette's own near-ground colour (white-ish
    on light, `pitch` on dark, the ladder's `scrim` step), the hover step goes *down* from raised
    on light as it goes up on dark, and ink on an accent is still `ink_over`'s pitch-or-paper pick.
    The waveform, spectrogram ramp (`surface` through the accent to `text`), visualiser and
    selection wash are all drawn from palette colours and need nothing of their own. The launcher
    icon is recoloured from the worn palette's accent as ever (`AppIcon::of(theme::worn())`).
  - **A fresh install wears the palette's own accent** (`native`; `Appearance::DEFAULT` names none);
    the Colour card offers it as a ringed swatch ahead of the seven; choosing none again =
    `Setting::Accent(None)`, removing the `accent` key. Theme cards are `theme_swatch()` wide, five a
    row in a `settings_column()` card.
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
  through a fixed lightness ladder (`Ladder`: `AFTER_DARK`, `BY_DAYLIGHT`, `IN_FULL_LIGHT`; a recipe
  names its own), then all seven accents at the canonical hues. Midnight, Graphite, Plum, Resonate
  Light and High Contrast Light are recipes. Flavours are `static LazyLock`s. `hsl` is pinned by
  `a_ramped_palette_reads_back_the_lightness_it_was_asked_for`. The five share the seven *hues* (an
  accent is what a colour *means*) but each lights them via its own `LitAt`
  (`each_ramped_palette_lights_the_accents_its_own_way` for the dark three; `LIT_ON_PAPER`,
  `LIT_ON_WHITE` for the light two, which the contrast tests hold).
- **One reading face, figures in tabular form; a second face only for codes.** Inter draws text and
  every figure: `theme::ui` carries `tnum`, so clocks, rates, counts and bitrates line up across rows
  without a monospace look, and `kit::figure` (body face, `text_xs`, muted) is the one way to draw
  one. JetBrains Mono (`theme::mono`) is drawn only through `kit::code`, for a string read character
  by character: the sync pairing code, the inspector's MusicBrainz ids, ISRC, barcode and catalogue
  number (`inspector::codes`) and a container's box kinds. Neither face is embedded: every package brings
  them (`packaging.md`), and a build run from the checkout finds what the machine has (on macOS the
  application carries them: `fonts::bundled` reads every font file of `Contents/Resources/Fonts`
  beside the executable into `TextSystem::add_fonts` before the ladders are walked, and each ladder
  ends on a face macOS has, Helvetica Neue and Menlo), so **the drawn family is
  settled once against the machine**: `fonts::settle` walks a ladder per face (Inter, Adwaita Sans,
  Cantarell, ...; JetBrains Mono, Adwaita Mono, ...) against `cx.text_system().all_font_names()` at
  start; neither ladder offers a family the other does (test); a machine with none keeps the wanted
  family. Not a setting; the About card reports the two (`fonts::drawn_in`, *Body face* and *Codes*). `theme::text_xs()`
  through `text_title()` and `text_lyric()` are the whole type scale, set via `text_size`.
- **A sheet and a pop-up are each one surface from the kit.** A sheet's card is `kit::sheet`
  (`surface`, `border`, `rounded_xl`, the long shadow) over `kit::over_a_scrim`, headed by
  `kit::sheet_heading` (an 18 px mark in its colour beside a level-2 title at `text_lg`) and ending
  in `kit::sheet_actions` (right-aligned, the safe gesture first): the Listen, delete and party-offer
  sheets. A panel that floats at a press (a menu, the Sort pop-up) is `kit::floating` (`raised`,
  `soft_edge`, the short shadow, presses stopped). `kit::rule` is the soft hairline between a
  surface's parts. `kit::ledger(across, cells)` is figures that belong together on one surface: a
  `kit::section` cut into lines of `across` equal cells by hairlines (a short last line keeps its
  empty cells, so the columns hold), used by the Statistics counts, number ones and moments, the
  inspector's signal path and Wrapped's tallies. The sharing, matching and Wrapped cards and the playlist picker are `kit::sheet`s
  too (the picker's scrim keeps its own press, which leaves the naming), and the record card is a
  `kit::floating`. **A shadow is one of three casts and no view builds its own**:
  `kit::Casts::casting(Cast)` is `Long` (a sheet, the magnified cover, the drop card), `Short` (what
  floats at a press, the downloads panel, the other-devices card, a cover lifting under the pointer:
  album, playlist and artist cells alike) or `Slight` (a hint, a toast, the type-ahead and copying
  pills, the play mark on a playlist's art), all at one strength through `theme::shadow`
  (`a_shadow_is_cast_by_the_kit_and_no_view_builds_its_own` walks the views; `chrome.rs`'s rim and
  `focus.rs`'s inset ring are the two that are not casts). **A wash, a veil and a cover's edge have
  one name each**: `kit::chosen_wash()` under a chosen row (a selected track row, a radio row, a
  device, a band), `kit::mark_wash(shade)` under a mark standing on its colour (an avatar, the zone
  switcher while several zones stand, the remote's play mark; never a tinted tile behind an icon
  that only decorates: an empty pane's mark and a nav row's are drawn bare), `kit::cover_veil()` over a
  cover (the favourite mark's ground, a download underway), `kit::cover_edge()` the hairline round
  one, `kit::field_lit()` a field's lit edge, `kit::uncovered_mark(side)` the disc where no picture
  is.
- **A control is a `kit::button` in one of four tones; nothing draws its own.** `Tone::Primary` =
  accent fill, the one gesture a heading leads with (*Play*). `Tone::Outlined` = raised secondary
  (*Play next*/*Add to queue*, one pair one tone). `Tone::Destructive` = failure-edged, only for
  losing something for good. `Tone::Ghost` = everything else. `kit::icon_button` = the square
  version; `kit::badge` = a word in its colour (body face, `text_xs`, medium), never a tinted pill,
  in sentence case (*Default*, *In use*, *Pinned*); `kit::format_badge` = the codec in the lossless
  or lossy colour beside depth and rate from `format::quality` in `faint` (*24-bit / 96 kHz*, never
  `24/96`). **Labels are in sentence case everywhere**: eyebrows, section names, column headers,
  badges, readings (*Muted*, *Track*); a codec keeps its own spelling (FLAC). Nothing is uppercased
  at runtime.
- **A choice is drawn by what it is, never a row of pills.** A few short values, one held:
  `kit::segmented` over `kit::segment`s (a statistics window, a Wrapped year, a lyric reading, a row
  cap, a speaker's place). A list with a long name or an open end: `kit::pick`, the held value and a
  chevron, its press opening a `Menu` of `Menu::chooses` entries, a check on the held one and room kept
  on the rest (*Fit toward*, *Preferred measurer*, *Heard through*; `pick` on a band's kind opens the
  segments under its row). What switches the whole pane between
places of the same kind (the services the Services pane browses) is a `kit::tab_strip` of `kit::tab`s:
names at `text_base`, the chosen one in `text` at semibold over a 2 px accent underline
(`TAB_MARK`, faded by `shown_while`), the rest muted and lit under the pointer, no box around them.
Inside a sheet, which a menu would close, `kit::radio_row`s (the Listen
  sheet's microphones, *Default microphone* first). **The Sort pop-up is small and anchored, not a sheet over a scrim**: every sort mark opens through `in_the_pane_ring_at`, `order_a_listing` keeps its corner in `RootView::ordering_at`, and `order_sheet` stands a `theme::order_width()` `kit::floating` panel there (at most the window's height less its margins, read off `Window::viewport_size`: a percentage there resolved against less than the window and cut the list short) in an `anchored` layer whose occluding ground closes it on any press outside (`the_sort_mark_opens_a_pop_up_under_itself_that_orders_the_tracks_and_leaves_by_every_way_out`). It reads as the menu it is: a faint heading (*Sort songs*), the orders as `kit::checked_row`s (a menu row's height and face, the held order in `text` at medium weight behind an accent check faded by `shown_while`, the rest muted and lit under the pointer, no wash under the held one), a `kit::rule`, then the direction as two `kit::segment`s worded for the order held (*A to Z*), no close or *Done*. Orders and direction are each a radio group a reader hears named (*Order*, *Direction*) with their keys. **Its keys are its own**: the focus panel carries `ORDER_CONTEXT` and `answering_in_the_order` binds after the window's lists, so at the same depth its keys win, with the panel or one of its controls focused (`the_arrows_in_the_sort_pop_up_are_its_own_before_they_are_the_windows`). Up and down step the held order round, home and end land on either end, left and right take either direction, letters typed jump to the order they begin (`typed_onto` over `typing::jumped`: one letter typed again walks on through the orders it begins, `RootView::ordering_typed` running out after `HELD_FOR`), enter closes; a key applies at once as a press does (a radio group's arrows choose), the list scrolls to the held row (`ordering_scroll`) and a ring standing on an order or segment moves to the one now held (`carry_the_ring_onto`) (`the_sort_pop_up_is_walked_by_the_arrows_turned_round_by_left_and_right_and_typed_into`). Several held at once: `kit::checkbox`es in a
  fixed-width grid (Filters). A value moved by a step: `kit::stepper` holding two `kit::step`s and a
  `kit::stepper_reading` (the speaker's degrees; `stepper_reading_put_back` where a press restores it,
  the lyric timing).
- **Pressability is the builder's decision; a greyed control carries no press at all.** `kit::Press` =
  `Takes` | `Greyed`; `kit::button_when` and `kit::mark_when` read it. `Greyed` draws at `kit::GREYED`,
  takes `cursor_default`, no hover style, pointer cursor, listener or focus ring: it must be decided
  where the element is built, as gpui's `hover` has `debug_assert!(hover_style.is_none())`.
  `settings::action` returns the bare greyed button on `busy`, else attaches listener and focus ring
  (`in_the_ring`). **A control that takes a press gives under it**: `kit::GivesUnderAPress` is gpui's
  `active` laying `theme::pressed()` (the hover ground a step further toward the text, held readable
  by `a_pressed_ground_is_a_step_past_the_hover_and_still_read`) under a ghost, outlined or
  destructive button, a mark, a pick, a stepper's step, the way back, a nav row and a place link,
  and a primary button thins its accent a step past its hover. Only the kit applies it; a row or a
  choice takes none, since its change of state is the answer and a row's press may begin a drag.
  - **Adding and forgetting a folder are not greyed: they wait.** `LibraryModel::add_roots` and
    `forget_root` put the request in `roots_waiting` (`RootWaiting::ToAdd`/`ToForget`) wherever a
    `Work` runs; `take_up_what_waited` runs when a pass hands its `Work` back: a forget first, one at
    a time, then every waiting folder in one scan. A forget mark is keyed by its root, and **armed by a first press** like *Reset everything*:
    `Armed::ForgetTheFolder` names the root asked about (the mark says *Press again to forget*), a second
    press on the same root calls `forget_root`, any other press or leaving the pane lowers it
    (`lower_every_armed_press`).
  - **An enrichment is deliberately not a `Work`**: `LibraryModel::enriching` is a slot of its own,
    started with `for_the_pass` (`online.md`). A running lookup greys *Look up*, *Refresh all* and
    *Look for missing covers* via `held_back`; *Speed up once* (`LibraryModel::hurry_the_lookup`) hurries the running lookup, or starts a hurried one, and reads *Sped up* greyed until it ends (`analysis.md`); a scan can still start under one. `RootView::enrichment_status` sits
    above the sidebar's *Settings* row while `LibraryModel::shows_the_lookup` (a globe in `theme::faint()`
    and *Enriching…*; a press opens the Library category's *Rescan, refresh and retag* page, `Page::Enrichment`); the toast `looked_up` tells what was found.
    **A lookup the window begins on its own is quiet** (at start, after a scan or a poll, carrying
    on an unfinished one: `Prompted::OnItsOwn`): the row and its announcement wait
    `A_QUIET_LOOKUP_SHOWS_AFTER` (3 s), so a pass with little due never flashes, and it ends with
    no toast unless the reference stopped it; what it found stays in *The last lookup*. One the
    listener asked for (*Look up*, *Refresh all*, a track's lookup, missing covers) shows at once
    and tells what it found (`a_lookup_begun_on_its_own_tells_nothing_unless_the_reference_stopped_it`).
    **The Library category's *What online knows* group shows the breakdown** (`library.md`): while a
    lookup runs *The lookup so far*, read from `EnrichProgress::snapshot` on the poll, and once it
    ends *The last lookup*, kept in `LibraryModel::looked_up` until the next begins (a section a
    row, only its counted outcomes: `lookup_lines`), with a note where it was stopped or the
    reference could not be reached (`lookup_ending`). Unlike the other passes' counts it stays
    after the toast, since the toast says only the albums and artists answered.
- **New songs always owe a lookup while automatic enrichment is on.** A successful scan that
  brought songs and a provider poll that kept any call `LibraryModel::enrich_new_songs`, gated by
  `after_scan` and `can_enrich`. A running lookup reads its queue at the start, so such arrivals
  set `enrich_owed`; its successful completion consumes that flag into one further pass. Stop
  clears it, and cancellation or an unreachable reference never starts a retry loop. Startup with
  no unfinished pass and turning Online on also ask for due metadata, covering songs already in
  the catalog that no lookup reached. `a_song_scanned_during_a_lookup_gets_its_metadata_in_the_following_pass`
  holds the first lookup open while a real scan adds another song, then checks that song's
  recording identity lands after it is released.
- **What a listener reaches for is what the lookup asks next.** `LibraryModel` holds one `Sought` for
  the run and hands it to every `enrich`. Set via `LibraryModel::ask_about`: scoping the tracks pane
  to an album or artist, playing a row, and `ask_about_what_is_drawn` whenever a listing is read with
  an artist scoped. Only reorders what the pass has left: a nudge, not a command (`library.md`).
- **Every pane opens with the same heading; a page inside a category opens with a hero.**
  `kit::heading` holds a `kit::heading_row`: an eyebrow only where it says something the title does
  not (*Album*, *Playlist*, *Add songs*, *Settings*, *Search*; never the sidebar group a pane sits
  under, nor *Lyrics*, *Visualiser*, *Analysis* or *Inspector* over the song's title), `kit::title`,
  `kit::subtitle` (counts, total length), `kit::actions` on the right; under it the *Reads* chips
  (`listing::reads`) and a naming row where a pane has one.
  - **A body under the heading pads one measure on every side**: `p_6`, the heading's own `px_6`,
    so statistics, the visualiser, analysis, the inspector, the party page and the suggestion shelves
    leave as much above and below as beside them.
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
    **With shuffle on, a row pressed in a search of the whole library (nothing scoped) plays the whole library from that song** (`with_the_whole_library`), shuffle left on, so a search picks where the shuffle starts and does not narrow it; a row on an album or artist page the box still narrows plays that page's listing, and a pressed song the library read does not hold (`is:hidden`) falls to the listing drawn (`play_from_the_listing_read_whole`). `RootView::play` clears the playing-playlist mark that only `play_playlist` sets.
  - An album adds the info mark (and the download mark, *Get the rest* on its hint, where tracks are missing); an artist adds one where
    it has genres, and the Albums/Tracks choice at the row's right. Play next, Add to queue, Add to
    playlist are not on the page. The row does not wrap (taffy sizes a wrapping flex item at
    min-content width).
  - The opened playlist uses the same hero, cover = `views/mosaic.rs` art, beside it a faint line of
    when it was made and last played; Play and Shuffle; *Add songs*, *Sort*, *Tidy* for a list; the
    pin; `Icon::More` opening the index card's right-press menu. The title opens the shared rename
    field. `kit::way_back` reads *Playlists*, above eyebrow *PLAYLIST* or *SAVED SEARCH*.
  - **A title that is a link is `kit::linked_title`, never `kit::title` over an `opens`** (the link got
    no width). Such a link is cut before drawing at the room its parent measures:
    `RootView::playing_heading` (lyrics, visualiser, analysis headings) measures into
    `heading_room` a frame behind and cuts with `kit::cut_to_fit`; the inspector cuts at
    `inspected_room`; `by_line` cuts the artist at its room, the album taking the rest. The artist
    under a title is `kit::linked_subtitle` (`kit::subtitle` over an `opens` stretched the link across
    the column).
- **A track row is the same eight cells wherever drawn.** `listing::columns` = the header the tracks
  pane, queue and opened playlist put over their rows; `number_cell`, `title_cell`, `artist_cell`,
  `format_cell`, `heard`, `length_cell` the cells under it, so no two disagree about a width. Title and
  artist give way together (`title_room` and `artist_room` start from `theme::row_artist()` and both
  shrink, only the title growing). A title carries no badge, neither an explicit mark nor a count of
  other copies (those are reached from the row's menu); text cells end in an ellipsis. The playing row draws `listing::playing_mark`
  in the number cell and its title in the accent. Row controls sit in `browser::row_controls`,
  **always drawn, quiet until their row is under the pointer**: `row_controls` draws them at
  `ROW_CONTROLS_AT_REST` (0.4) and whole on `group_hover(ROW_GROUP)`, so a listing is not a wall of
  marks and a keyboard ring landing in one still shows where it stands. They were once hidden until
  hovered or focused, but gpui's `in_focus` styles an element *inside* the focused one, so they
  showed on every row while the window itself held the caret and vanished the moment a field did
  (the search box, the playlist picker's name field), which read as a bug (found on a hosted KWin,
  the `drive-a-drag` skill's compositor). Every row that holds them carries `ROW_GROUP` (a served
  row too). The star (`favour_mark_under`) alone still waits for the hover.
  - **FORMAT is a sort column** on the tracks pane and Favourites (`Sortable::Format` →
    `SortOrder::Format`); the queue and an opened playlist keep it a plain header, `RowOrder` having
    no format order.
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
- **A part leaves out what the file does not declare; the inspector is a specification sheet in one
  column.** `fields` takes an `Option` per row and draws only what is there, one faint line where a
  whole part is empty. The signal path is a `kit::ledger` of three cells (Source, Processing,
  Output); under it each part (`inspector::part`) is ruled off from the one above and named in a
  `PART_MARGIN` (176) margin beside its first row (`margin`, where *Output* also wears its mode's
  badge), its rows a property list to the right: no boxes, since eight bordered cards of key and
  value read as a dashboard and the sheet reads as what it is. The analysis pane's cards
  (`inspector::card`, `beside`) are still cards. The inspector stacks the signal path, Source,
  Output, Bitrate (and its graph), ReplayGain, Heard, Container layout and Tags in one column at most `theme::inspector_column()`
  (760) wide, centred in the pane (`mx_auto`, so a wide window leaves no gap on one side alone)
  under the pane's `kit::heading`, which holds the song's title and by-line where the lyrics,
  visualiser and analysis panes hold theirs, so the title stands still from one to the next:
  dealt into two columns, cards of one row stood beside cards of eight and left ragged gaps. `beside` (two `flex_col` columns wrapping to one) is the analysis pane's, for its Levels and
  Source.
- **The inspector is the one pane reading the catalog, not the engine; the *Heard* card is why.** Every
  other card is the `StreamDigest` or `OutputStatus`; play count and last play come off the `Track`
  `Playing` already resolves through `LibraryModel::track_of` (no query). A row no scan has seen
  answers *no library row*, not zero. `format::since` reads dates as the largest span that fits.
- **What a search matched is lit where drawn, by one element.** `listing::matched` answers the text
  itself where nothing is lit, else a `StyledText` with `HighlightStyle` runs (a lit cell still
  truncates, wraps, measures as one string). Runs are `Search::lit` for that cell's `Column`
  (`library.md`), so a fold-reached name lights the spelling drawn. Only browse panes call it. A run
  is the accent at `FontWeight::MEDIUM`.
- **The albums pane is a grid whose column count follows the window in the frame it moves, and
  whose cells fill its width.** A `uniform_list` row is `columns` cells of `RootView::grid_side()`:
  as many `theme::grid_cover()` cells as fit (`browser::columns_across`), each then grown to share
  what is left (`side_across`, at most `A_CELL_GROWS_AT_MOST` of the cover), so the grid ends where
  the heading's actions do and never ragged short of the edge; `grid_row()` is the row that side
  makes. The picture is still decoded once at `Drawn::InAGrid` (twice the cover's device pixels,
  which a grown cell stays inside), so a resize redraws and decodes nothing. The artists and
  playlists grids, an artist's records and the suggestion shelves take the same side, each pane
  holding a `kit::measures_the_grid`. Columns read `RootView::grid_width`,
  a `kit::GridWidth`: the canvas under the list (`kit::measures_the_grid`) writes on prepaint how much
  *narrower than the window* the grid is, and `RootView::region` tells it the window's width whenever
  the pane is built, so a resize draws at the new count the first frame
  (`a_grid_is_as_wide_as_the_window_now_less_what_it_was_narrower_by`). **The list is not built until
  that cell holds something** (the `max(1)` fallback stacks full-width cells); the canvas is
  `absolute` and sized by the container, so it measures the same with the list there or not. A cell's
  cover is `Drawn::InAGrid`.
- **An empty pane says what is missing and what to do, in the Listening Party welcome's shape**
  (`kit::empty`): the pane's icon drawn bare in `faint` (`kit::empty_mark`, the same mark the
  party's welcome draws; no disc, no wash behind it), a short headline naming the state (a level-2
  heading at `text_lg`, no full stop: *No playlists yet*, *Nothing playing*, *No albums match*), and
  a sentence naming the gesture that fills it (`text_sm`, muted, ending in a full stop), every empty
  state having one. A pane whose heading already leads with the gesture offers it again in the
  empty state as `Tone::Outlined`, never a second primary (*New playlist*), and counts nothing in
  its subtitle while it holds nothing. A failure (the library unread, a decode that stopped, a server or MusicBrainz out of reach,
  a lyric source that did not answer) is
  `kit::trouble`/`trouble_offering`: the same shape with the mark in `theme::failure()`. **A search that matched
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
- **A hint names its key one way, and the key is the one bound.** `keys.rs` is `chord!`
  (`chord!(queue)` is a `Chord`: what a Mac presses and what every other system does, both
  literals) and `key!`, the same chord as a hint shows it (`key!(play_pause)` = `space`);
  `app::bindings(system)` binds `chord!(..).on(system)`, the system being `cx.platform_info().os`.
  The wording is what the key does, an em dash, the ways (last two joined by *or*): `Louder — the
  wheel or ctrl-up`; a hint saying more joins such sentences with full stops, a described state
  first. A hint from a catalog takes each key as a variable (`play-hint = Play — { $keys }`, said as
  `words::transport::play_hint(key!(play_pause))`), so the key stays the bound one and the words
  around it translate; `keys::keyed(what, &[ways])` writes the same shape through `keys.ftl` where
  the words come from elsewhere; the keys written in the search field, the settings filter and the
  search field's menu are `key!` too
  (`every_key_a_hint_names_is_a_key_something_is_bound_to_on_every_system`).
- **A Mac steers with Command where the rest steer with Control.** A chord written `secondary!` is
  `cmd-` on macOS and `ctrl-` elsewhere, and what is written `ctrl-` stays Control everywhere: mute
  (`ctrl-m`), shuffle (`ctrl-h`), choosing a reached row apart (`ctrl-space`) and the panes
  (`ctrl-tab`, `ctrl-shift-tab`), whose Command forms are the system's (Minimise, Hide, Spotlight,
  the app switcher). macOS alone binds `answering_as_a_mac_expects`: Hide (⌘H), Hide others
  (⌥⌘H), Minimise (⌘M), Close (⌘W) and Full screen (⌃⌘F); ⌘Q is the quit key and ⌘, the settings
  filter (`secondary-,`: gpui names that key `,` on every system, so the `ctrl-comma` once bound
  answered nothing). In a field a Mac moves and deletes by word with Option, reaches either end
  with Command and its arrows, and takes Emacs' `ctrl-a`, `e`, `b`, `f`, `d` and `h`
  (`field::edited_as_a_mac_expects`). A press that chooses rows apart reads
  `Modifiers::secondary`. **A hint shows a Mac its keys in Apple's marks and Apple's order**
  (`keys::shown`: `⌘U`, `⇧⌘Z`, `⌃M`, `⌥↑`, *Space* from `keys.ftl`; one `OnceLock` per `key!`),
  and is the bound string itself everywhere else, byte for byte what it was. **Both spellings are
  literals chosen at run time, never `KeyBinding::load`**: `load` resolves gpui's `secondary-` but
  also maps every key through the platform's keyboard mapper, which `KeyBinding::new` does not and
  which would move what Windows binds, and two literals let the tests read the Mac's keymap on any
  machine (`the_chords_a_mac_keeps_for_itself_do_what_the_system_says_and_nothing_else`,
  `a_mac_steers_with_command_and_leaves_control_to_the_chords_command_cannot_take`,
  `nothing_a_mac_alone_binds_is_bound_anywhere_else`,
  `a_mac_is_shown_its_keys_in_apples_marks_and_apples_order`,
  `on_a_mac_command_steers_the_window_and_control_is_left_to_the_chords_command_cannot_take`,
  which opens the window under `TestAppContext::set_platform_info`). The tests press
  `secondary-` chords, which gpui's `simulate_keystrokes` resolves for the machine they run on.
- **A control names itself through one seam, and the seam lets go.** `hint::Names` is the only place a
  tooltip is attached (a test walks the crate's sources): gpui's plain `tooltip`, never
  `hoverable_tooltip` (neither press nor scroll takes that one down). A hint gpui had already scheduled
  from a move survives the frame that drops its builder, so `Hint::render` draws nothing while
  `asked()` says no and a hint cannot come up over a sheet its own control opened
  (`a_hint_does_not_stand_over_the_sheet_its_own_control_opened`); every sheet in
  `something_stands_over_the_pane` silences hints. `hint::asking` is one per-thread flag (the
  window draws on one thread, and a test's flag is its own)
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
- **A control in the ring is named on keyboard focus as it is named on hover.** gpui raises no
  tooltip while the last input was the keyboard, so `Names::names` also remembers the wording under
  the element's id (`hint::wording_of`, a thread-local bounded by `NAMED_AT_MOST`, cleared when it
  fills), and each ring wrapper (`in_the_ring`, `in_the_ring_at`, `option_in_the_ring`, the pane and
  standing rings) hands the control to `Controls::captioned_by_its_name`, which keeps the wording
  under the handle's name beside the handle (pruned with it in `opening`). `RootView::render` asks
  `focused_control_named` (keyboard modality, a handle of `controls` or `standing_controls` focused)
  and draws `hint::caption`: the hint's plate, bottom left above the playback bar, clear of the
  centred toast. A control built without a name (a field, a row with no hint) shows nothing
  (`tabbing_onto_an_icon_only_control_names_it_where_a_pointer_would_have_shown_a_hint`).
- **A screen reader hears the window through gpui's AccessKit tree, built only while one listens.**
  gpui 0.2.2 (the pinned zed rev) publishes a node for every element with an `id` *and* a `role`
  over `accesskit_unix` (AT-SPI) on Wayland; text children make no node, so every node carries its
  own label. `views/spoken.rs` is the one vocabulary: `spoken::listened_to` is a thread-local written
  once a frame by `RootView::render` from `Window::is_a11y_active`, and every `Spoken` builder
  (`said_as`, `named_as`, `spoken_as`, `thrown`, `chosen`, `listed_as`, `placed`, `keyed`, `told`,
  `read_as`, `greyed_for_a_reader`, `chosen_among`, `pictured_as`, `linked_as`, `read_as_typed`,
  `focused_while`) is a no-op while it says no, so a frame with no reader formats
  and allocates nothing for one.
  - **A control is named by its hint up to the keys**: `said_as` labels it with what precedes a hint's
    em dash (`named_by`: *Play — space* is *Play*) and describes it by the whole hint where that says
    more; `named_as` takes a visible label and keeps the hint as the description. The kit names its
    own: `kit::button`/`button_when` and the marks are buttons (greyed = AccessKit's disabled, through
    `a11y_synthetic_children`), `star` a toggle button, `segment` and `radio_row` radio buttons, `pick` a combo box, `checkbox` a
    check box;
    `kit::switch_row` and the Listen sheet's *Keep listening* are the switch, never the
    `kit::switch` mark inside them.
  - **What is spoken**: the playback bar is a group *Playback* holding *Now playing: title, artist*,
    previous/play-pause/next, shuffle, repeat and queue as toggles, the sleep control, mute, and the
    seek and volume rails as sliders (`read_as`: 0 to 100 with the said value, *1:23 of 4:56* or
    *40%*; Increment/Decrement step the rail as its keys do, SetValue lands it via
    `RootView::set_by_a_reader`); the sidebar is a navigation landmark of page tabs, the one in front
    selected, pinned playlists links; a track, queue or playlist row is a list item named
    `spoken::song` (*title, artist*), selected where `marked_where_reached` (or the queue/playlist
    reach) says, the queue and playlist rows placed *n of m*; a `Field` is a text, search or password
    input labelled by its placeholder, its value the text unless masked; a menu is a menu of items
    with their keys, placed, the reached one selected; a toast is an alert (`Trouble`, assertive) or a
    status (polite), keyed `("toast", serial)` so each notice is a new node and is announced.
  - **A menu reached by key is followed by the reader.** gpui's `aria_active_descendant` is honoured
    only where the focused node is an ancestor on the a11y stack, and a `deferred` menu is built
    after the root's stack has unwound, so the window's focus never qualifies. While a reader
    listens and a menu has a reached entry, `RootView::hand_the_reader_the_menu` (first thing in
    `render`, after `listened_to`) moves keyboard focus to `menu_focus`, tracked by the menu panel;
    the reached entry claims `focused_while(reached)`, so the reader's focus follows every step of
    the arrows; once the menu is gone it hands focus back to `RootView::focus`. `Handing::of` is
    the whole decision; a menu opened by pointer has no reached entry and moves nothing, and with
    no reader listening focus never moves (`a_menu_reached_by_key_is_handed_the_reader_and_the_window_gets_it_back_once_it_closes`).
  - **Cells, rows and links are named by what they draw.** An album cell is a list item *title,
    artist* (`spoken::song`), an artist cell or list row *name, n albums, n tracks*
    (`spoken::artist`), a found artist or an album not held likewise (the latter *title, not in your
    library*), each selected where reached. A link is `pressed_to`'s `Role::Link` labelled by the
    words it draws: its label is `impl IntoElement + Spelt`, and `listing::matched` returns a
    `Matched` that is both the drawn element and its spelling, so a lit search match still names
    the link whole; an album cell's artist beside a year is cut by `cut_to_fit` to the cell's width less the dot and the year (`artist_room_beside`, `listing::matched_within` clamping a lit run to what stayed), so the link is as wide as its text and the dot stands against it (`an_artist_cut_by_an_ellipsis_in_an_album_cell_leaves_no_gap_before_the_year`); the bar's and inspector's links draw what `cut_to_fit` left, an ellipsis
    included, and are named whole: it returns a `kit::Cut` (the drawn text, the whole spelt), the
    same pair as `Matched`. A lyric line is a list item of the *Lyrics* list (`lyric-sheet`),
    *n of m*, selected while in play; a line held open far from the pane keeps its node (empty box,
    same id) so the list is whole.
  - **A choice is a radio button wherever it is drawn**: `kit::choice_row`, `dot_swatch`, the palette
    cards and the music folders' circle go through `Spoken::chosen_among` (label, a description built
    only while listening, toggled where chosen); a device describes how it is reached and what it
    takes (`sink_spoken`).
  - **A plot is an image saying what it shows** (`Spoken::pictured_as`: the wrapper div's id, a label
    and a description built only while listening), and the description carries the values the eye
    reads: the equaliser curve its range and each band (*n, peak at 2.5 kHz, -3.0 dB*, `off` where
    switched off; `curve_said`), the waveform its hint with the peak, loudness and dynamic range
    (`waveform_said`), the spectrogram and average spectrum the cutoff with its drop and the
    verdict, the spectrogram how far up and how long it reaches, the spectrum its mean level under
    1 kHz, to 4, to 10 and above (`spectrogram_said`, `spectrum_said`, over `cutoff_said`), the
    bitrate graph its floor and ceiling. The visualiser says its mode alone: its values move every
    frame and would be re-announced.
  - **A chart, a card and a run of missing rows are named by what they say.** The Statistics chart
    is the list *Listening by day* (`statistics::listed_days`), each bar a list item placed *n of m*
    labelled by what the hover names (*yesterday · 3 plays · 1h 0m listened*); *Listening by hour*,
    *Listening by weekday* and *Genres most heard* are lists alike, and a ledger cell says its name
    and reading (`read_plainly`). A Suggestions card is
    a list item (`("suggestion-card", index)`) of *name, reason, tracks · length* (`said_of`),
    selected where reached. The Missing pane's rows are list items: an album heading *title, owner,
    n missing*, a disc *Disc n*, a track `browser::unheld_said` (*title, artist, not in your
    library*, the one every unheld row, on an album or in a search, takes), an artist heading
    *name, n releases*, a release *title, kind, year*. `spoken::joined` makes the comma list and
    drops an empty part.
  - **A field's caret and selection are a synthetic text run.** `Spoken::read_as_typed` gives the
    field's node one `TextRun` child holding the text (character lengths in bytes) and sets
    `text_selection` from `Edit::anchor` and `Edit::cursor`, converted to letter counts
    (`Typing`); a masked field exposes none, so a secret's length and caret stay unspoken.
  - **No two nodes share a global id**: gpui panics in a debug build on a duplicate (release drops
    the node), so a role goes only on an element whose id is unique under its parents.
  - **The four `Part`s draw uncached while a reader listens**: a reused cached view replays hitboxes
    but not AccessKit nodes, so a region nothing notified would drop out of the tree that frame;
    `part::cached` lays the part out in a plain div of the same style instead.
  - Held by `views/spoken.rs`, `kit.rs` and `transport.rs` tests reading the built element's
    `Element::a11y_role`/`write_a11y_info` (`spoken::node_of`); gpui's test platform never activates
    the tree. Heard for real once over AT-SPI: a hosted `kwin_wayland --virtual`, the app on a
    private session bus with `at-spi-bus-launcher` and `at-spi2-registryd`, `org.a11y.Status
    IsEnabled` set, read with python's `Atspi` (a tab clicked through the Action interface changed
    pane, the volume set through the Value interface moved); no screen reader (Orca) was run. The
    cells, links, lyric lines, plots, bars, cards, the caret and selection and the menu hand-over
    were only built and read back in tests (`docs/TESTING.md`).
- **The focus ring is the keyboard's, hollow, drawn inside the control.** Every ring wrapper styles
  through `focus_visible` (`focus::lit`, `picked`, `lit_over`), never `focus`, so a click focuses
  without ringing and only Tab or an arrow lights it (gpui's `last_input_was_keyboard`), and none
  puts a fill under the control. `focus::ring_of` is an inset shadow: gpui's shader paints an outset
  shadow as a solid rectangle behind the element, so a control with no ground of its own (a rail
  row, a segment, a ghost button) was filled with the ring colour. An inset shadow is painted before
  the element's children, so an opaque ground child (the `shown_while` wash of the settings rail,
  `kit::segment`) is laid by `focus::beneath_the_ring`, inside the ring's
  width. A field holding the caret still lights its border, since a click into it is where typing
  goes, read from `Field::is_focused` (`RootView::finding_holds_the_caret`, set as the pane is
  drawn), never gpui's `in_focus`, which lights what stands *inside* the focused element: the
  window's root holds the focus at rest, so the settings filter stood lit in the accent whenever no
  control held it.
- **What is drawn at rest wears a soft edge; `outline` is for the two marks that are nothing but
  their edge.** `outline` (`DRAWN_EDGE`, 3:1) now draws only a switch's track and a radio's ring. A
  button's edge is `border`; everything else that once took `outline` (the search field at rest, a
  menu, hint, toast, the record card, the seek and volume rails' and a scan's track, a
  meter's ground, a plot's zero line, a day with no plays) takes `theme::soft_edge()`, `border` a
  quarter of the way to `outline`: at 3:1 each drew a hard bright line, and a surface that floats is
  told by its raised fill and shadow, a rail by what fills it, a field by its fill, mark and words.
  In High Contrast `border` is at its 4.5:1 bar, so the soft edge is too
  (`a_soft_edge_is_quiet_beside_the_outline_and_strong_in_high_contrast`).
- **Accessibility beyond the tree: the bars and what holds them.** The contrast tests in `theme.rs`
  also walk every theme x accent for: `outline` (`DRAWN_EDGE`) standing off the panes it is
  drawn on at 3:1, `theme::focus_ring()` (`FOCUS_RING_WIDTH` 2 px, full strength, 3:1 against every
  pane; `focus_ring_over(fill)` picks whichever ink stands off a fill), every accent as text on the
  panes, every semantic colour as text on its own `tinted` wash, the inks on every pane they are
  drawn on, text on the selection wash, and High Contrast at the stricter ratio. A colour that fell
  short was lifted along its hue, as `THIRD_INK` is. A switch thrown off wears its knob in
  `theme::switch_knob_off` (`ink_over` the `outline` track, 3:1 held by
  `a_switch_knob_thrown_off_stands_off_its_track`), since `faint` on `outline` read at 1.3 to 1.5:1. Fixed heights are measures read through
  `scaled()` (`BUTTON_HEIGHT`, `PILL_HEIGHT`, `WAY_BACK_HEIGHT`, `COLUMN_HEADER_HEIGHT`,
  `SIDEBAR_ROW`): `a_view_draws_no_height_in_a_bare_number_of_pixels_unless_it_is_fixed_by_design`
  fails on a new literal. `TextSize::Largest` (twice the middling size) is the 200 % step; a hit
  area under `TARGET_AT_LEAST` (24 px) does not compile (`RAIL_HEIGHT` is 24).
  - **Static text and landmarks.** `Spoken::headed(level, label)` (a heading), `read_plainly` (text a
    reader can land on) and `dialog` (`Role::Dialog`) join the builders, no-ops while nobody listens.
  - **A sheet is a dialog with its own focus** (`views/sheets.rs`): the delete sheet, Listen sheet,
    playlist picker, magnifier, record card and the party offer (*Join this listening party?*, landing on *Cancel*, `party.md`) name a `Sheet`; opening one remembers the opener and
    lands focus on the safe control (Cancel for a deletion, no key confirms), Tab steps inside it
    (`step_the_focus`), and closing returns focus to the opener.
  - **A click has a name and a tab stop.** `views/audit.rs` scans `views/` for `.on_click(` sites that
    are on no element a reader can name and fails on a new one; the sites that remain are listed in it.
  - **Announcements are rate-limited** (`views/announcing.rs`, `ANNOUNCED_AT_MOST_EVERY` 5 s): scan,
    enrichment and download progress and a search's result count speak politely once in that span. A
    toast's timer waits while it is hovered or focused.
  - **Motion and contrast follow the desktop until the listener says otherwise.**
    `resonate_mpris::desktop_preferences` reads `org.freedesktop.appearance` `contrast` and
    `reduced-motion` (GNOME's `enable-animations` as the fallback) within 300 ms; the binary seeds High
    Contrast and Reduce motion from it only where `theme` / `reduced-motion` are not stored
    (`Config::appearance_where_the_desktop_asks`, `reduces_motion_where_the_desktop_asks`). The
    visualiser draws one still frame under reduced motion and has a pause key.
  - **The colour scheme and the text scale follow the desktop live, and a hand choice ends it.**
    The same read also takes `color-scheme` (1 dark, 2 light, anything else says nothing) and the
    text scale (GNOME's `org.gnome.desktop.interface` `text-scaling-factor`, else KDE's
    `org.kde.kdeglobals.General` `font` point size against its ordinary 10); `watch_the_desktop`
    listens to the portal's `SettingChanged` for those namespaces on a `resonate-desktop` thread and
    tells the window each changed `Preferences`, which the binary turns into a
    `resonate_core::DesktopLook` (`Bus::look` at start, `Bus::desktop` after). `Appearance` is the
    *chosen* look (`theme`, `accent`, `text_size`, `follows: Following { scheme, text }`) and
    `Appearance::seen_by(look)` the *worn* one: a following scheme wears `theme.on(ground)`, a
    following text size `TextSize::nearest(scale)` (the nearest of the five, so no layout is ever
    drawn at a scale the tests do not hold; below Small and above Largest it clamps), a desktop that
    says nothing leaves the chosen theme and the hand size as the fallback. `theme::chosen()` is the
    former and `theme::worn()` what is drawn; `theme::wear` takes the chosen, `theme::hear_the_desktop`
    the look, and each recomputes under the one lock. The cards and size choices mark the *worn*
    value; the two switches show the chosen `follows`. **Choosing a palette or a size by hand turns
    its following off** (writing `theme` and `follow-desktop-scheme = false`, or `text-size` and
    `follow-desktop-text-size = false`); turning a switch on or off keeps what is worn at that moment
    as the chosen theme or size, so nothing jumps. **Default**: a switch is on where its choice is not
    stored (`Config::appearance_where_the_desktop_asks`: `follow-desktop-scheme` true where `theme`
    is unset, `follow-desktop-text-size` true where `text-size` is unset), so a fresh install on a
    light, scaled desktop wears Resonate Light at the nearest size and an install with a stored
    palette or size is as it was. Held by `the_desktop_light_wears_the_counterpart_and_its_scale_the_nearest_size`
    and `the_desktop_is_not_heard_where_the_listener_wears_a_palette_and_size_of_their_own`.
- **A playlist row no scan has seen draws as a queued one does.** `views/listing.rs` owns the `Row`
  the queue and playlists panes build: the scanned `Track` where the library has one, `Player::media`
  where not, the file stem only where the file cannot be read. The play count rides on the same `Row`
  (`listing::heard`: one `theme::row_plays()`-wide cell for the tracks pane, both list panes and the
  playlists index); `how_often_and_how_lately` folds the date into it (*12 plays · 3d*, header HEARD,
  `COUNTS_AS_HEARD`); `format::age` fits it from the `SPANS` table `format::since` reads, each span
  naming its two messages in `words/en/format.ftl` (`every_span_is_named_both_ways_and_no_two_read_alike`).
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
  the bar; rises `RISE` (10 px), fades in over `ARRIVAL` (220 ms), lingers `LINGER_AT_LEAST` (6 s;
  `LINGER_IN_A_BURST_AT_LEAST` 2 s where more wait; a notice is read for `READING_A_WORD` (500 ms) a
  word where that is longer than its floor (`reading_time`, `lingers`); **no toast stands indefinitely**:
  a `Trouble` with nothing waiting lingers `TROUBLE_LINGERS_AT_LEAST` (10 s), longer than a plain
  notice, then goes (`a_trouble_lingers_longer_than_a_plain_notice_yet_never_stays`); a failure the
  listener did not ask for and cannot act on (a found song's cover not fetched) is traced, never
  toasted; at most `WAITING_AT_MOST` (4) waiting, never the same words
  twice (a `Trouble` told again while the same one shows or waits is counted on it, *… (×3)*, and a
  repeat of any other tone is dropped); a full line gives up its oldest non-`Trouble` for a newer notice, turns a plain one away where
  only troubles wait, and keeps every `Trouble`, so troubles alone may pass four), fades out over `WITHDRAWAL`; a click or escape takes it early. **The pill's drawing writes the `Toaster` only where its focus moved**: gpui's
  `default_global` queues a notice to the global's observers on every call and `RootView` observes the
  `Toaster`, so a write each frame redrew the window forever while a toast stood. A `Notice` is `Trouble`,
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
  not content, so `Region::laid_out` declares sizes. **The Windows header is always drawn whole**:
  gpui replays ordinary hitboxes from a cached view but drops its window-control hitboxes, so a
  reused header loses drag, minimise, maximise and close after its first frame. `part::cached`
  uses the same plain holder as while a screen reader listens, keeping the other regions cached.
  **The pane is held in flex columns, its root the one item** (`Region::holding`): width is the
  cross axis, stretched and never measured; a pane root
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
  buttons sit on the window's centre line. Centre: basis `theme::transport_centre()` (560), shrinks to
  `CENTRE_AT_LEAST` (260), gives way first. A press on the total clock toggles time left
  (`RootView::showing_time_left`, not kept; `format::left` subtracts whole seconds so elapsed plus left is the length shown, and a row opening (`Buffering`) draws and acts as *Pause*); the rail fills its row (`Handle::fills_its_row`). Both
  sides `overflow_hidden`. **The seek rail names the time under the pointer** in a bubble
  (`slider::Pointed`, `RootView::seek_pointed`; drawn, not a `hint::Names` tooltip; nothing where
  length is unknown).
  - **What does not fit is left out whole, never clipped.** `views/gives_way.rs` = arithmetic over theme
    measures, no gpui. Status cluster measured into `RootView::status_room` a frame behind;
    `status_kept` drops volume reading, sleep control, repeat, shuffle, then volume rail, until the
    rest fits; never the queue button or the speaker. `signal_kept` likewise against `playing_room`
    (`kit::width_of`): depth and rate give way; the codec badge stays.
  - **The bar is `theme::transport_height()` (88) on `surface` under a `soft_edge` rule, inset on each side by what the cover leaves above and below it (`bar_inset`, 16), and the centre column is padded at its top by the rail's room under the track (`RAIL_ROOM_UNDER_THE_TRACK`), so the play disc and the drawn track sit as far from the bar's top as from its bottom.** The play
    button is a 40 px disc in `text` with the glyph in `background`, dimmed to `kit::LIT` under the
    pointer (never turned to the accent); the steps are bare glyphs lit on hover. Shuffle, repeat and
    the queue are a glyph in the accent over a 4 px accent dot (`TOGGLE_DOT`) while on, the dot faded
    by the same flip, never a tinted chip; the running sleep timer is its glyph and countdown in the
    accent. The clocks are `kit::figure` in `faint`, time left with a true minus (*−3:12*).
  - **Shuffle says how it shuffles from a right or shift press** (`shuffle_button`,
    `shuffle_menu`): a plain press or `ctrl-h` toggles; a right press (`menu::opens_a_menu`) or a
    press with shift held opens a `Menu` of *Shuffle*, the three styles, the Smart switches where
    Smart is held, and *Shuffle settings…* (Playback). Every choice goes through
    `RootView::shuffle_as`, which the Playback category's *Shuffle* group uses too: one
    `Command::SetSmartShuffle` and a `Setting` per key that moved
    (`a_right_or_shift_press_on_shuffle_opens_how_it_shuffles_and_a_plain_press_still_toggles`).
  - **Now-playing panel**: cover (`theme::now_playing_cover()`, 56, rounded by `cover_rounding`),
    title, artist, album, signal path: the codec word in the lossless or lossy colour then depth and
    rate in `faint` (`kit::format_badge`'s shape, no pill; the quality lights to `muted` under the
    pointer, no wash), and nothing of the output: the bar names no mode (*Bit-perfect*, *Converted*),
    which the inspector's Output column spells out. A press opens the inspector. **The path is as wide as what it says**: `self_start`
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
    A content-sized flex item needs the room its parent offers to decide where to cut. Title and
    album go through
    `kit::cut_to_fit`: title at `RootView::playing_room` (a frame behind) less the star; album at the
    room `by_line` is handed less artist and separator (`kit::width_of` sums font advances; a glyphless
    character counts one em); no room left = artist alone. The inspector uses `inspected_room`.
  - **Every bar control names itself via `views/hint.rs`**: a step names its key, a toggle its state
    and what the key does next. A rail is filled in `TEXT`, accent under the pointer or held. The
    volume reading is always drawn, `theme::volume_reading()` wide (`opacity(0)` left a hole);
    `kit::figure` draws it, the same face as the clocks.
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
  The choice is a heading `kit::segmented` living for the run; the pointer over the column (`near_the_words`,
  via the `pointer_watch` idiom; an `on_hover` opened it from the empty gutters) or a scroll of your own
  opens it out: `LyricsModel::shows_every_line` weighs `following`, which runs out `HANDS_OFF` (6 s)
  after a hand scroll, and `spread` closes the sheet again over one `TURN`. An unsynced set has no lit
  line (`ADRIFT`, no choice). **The sung line brightens; it does not grow**: gpui floors a glyph's vertical
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
- **A sheet that runs early or late is moved by the listener, for that track.** A `kit::stepper` in time's order (`RootView::lyric_timing`;
  `LyricsAhead::STEP_MS` 100, within `MOST_MS` 30 s; `resonate-library`'s `sung.rs`): minus is sooner,
  plus later, and the reading between them is signed the same way (*−0.3 s*, `said_ahead`, though
  `LyricsAhead` itself counts how far the words run ahead), lit and putting the sheet's own timing back
  when pressed, faint at *0.0 s*, its width held so the marks stay under the pointer. `look_for_lyrics` reads `Library::lyrics_ahead` into
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
  `ends_in_an_ellipsis` = `whitespace_nowrap` + `line_clamp(1)` after `truncate()`. GPUI's no-wrap
  path checks the shaped width before truncating, so a content-sized name keeps every letter when
  it fits. The wrapped path sums individual character advances without kerning even for a one-line
  clamp, so a short name measured at its intrinsic width could become *AVAT…* instead of *AVATAR*
  despite spare room in the cell. The driven regression uses the platform text system rather than
  GPUI's fixed-width mock (`names_that_fit_stay_whole_and_long_names_end_in_an_ellipsis`).
  **No `truncate` stands alone** except two in
  `views/search.rs` (a quoted query inside `kit::title`, a section's faint hint line). **A content-sized
  box does not wrap**: `kit::KeepsItsWidth::keeps_its_width` (`flex_none`, `max_w_full`) sets
  `whitespace_nowrap`; those boxes are cut by `kit::cut_to_fit`; a `flex_shrink` cell keeps the clamp.
  GPUI remeasures a truncated layout for later unconstrained probes and preserves the original
  text runs between measures; neither the earlier no-wrap cache workaround nor mutable truncated
  runs apply to the pinned revision.

## Driven by tests

- **A pane is pressed, dragged and scrolled by a test under gpui's test platform.** `resonate-ui` has
  gpui `test-support` as a dev-dependency; `driven.rs` is the harness. `Driven::open` sets the
  `ResonateApp` global as `run` does (`Player` over an `Unplugged` backend; in-memory `Library`;
  `Ephemeral` settings; `Places` in a temporary folder) and opens a real `RootView` in an activated
  `VisualTestContext`. It presses, right-presses, drags, scrolls and moves the pointer at bounds gpui
  drew, read via `debug_selector`: `kit::Found::found_as` names every button, pick, segment, switch,
  choice and mark by `ElementId`; rows, tabs, menu entries, surfaces name themselves (`tab-visualiser`,
  `track-<id>`, `queued-<id>`, `menu-entry-<n>`, `picker-<id>`, `equaliser-curve`). `Driven::settle`
  runs the executor until it parks and advances the test clock a frame; `Driven::until` waits on the
  real engine and scan threads; `Driven::play` opens rows via `Command::Resume`, the first paused.
  **A `Driven` holds `theme::WEARING` shared for its life** (`Wearers`), and a theme test or a
  `Driven::wearing_alone` test holds it exclusively, since `theme::WORN` is one for the process and a
  test wearing another palette or a text scale moves every row another test measures.
  **A `Driven` dropped closes its window, lets what was pending run and drops the globals**, so
  the engine it started ends with its test: gpui leaks a test's `App`, each kept an engine's ten
  threads and a `Drawer`'s two, and a Mac refuses a process its two thousandth thread (`macos.md`).
  Microphones are planted via `ListenModel::hearing_of`; `Driven::recognising` hands the window a
  `Fingerprinters` whose fake printer hears the playing file as another song;
  `Driven::reaching` a `Reaching` naming the reference and providers. A control below the fold is out of reach of a press (`Driven::scroll` the
  settings body first); a switch is pressed by its `-track` mark. Every Output and Processing choice and
  switch is pressed and read back off `Player::output_settings`; `Driven::hearing` hands the
  `Unplugged` backend sinks to list, so a device row is pressed too. gpui can't build an
  `ExternalPaths` in a test: `dragged_over` and `dropped` are called on the `RootView`; the platform's
  own drag events are driven by the `drive-a-drag` skill on a hosted KWin, which `crates/resonate/tests/dragging.rs` runs (opt-in: `RESONATE_DRAG_TESTS=1`, a compositor, GTK 4 in python3 and `spectacle` on the path) and which proves the drop is accepted and copied and the overlay drawn: against `before.png` the window drew widely, and in `over.png` (decoded with the `png` dev-dependency, found by what is drawn rather than by the window's place, which KWin does not always honour) everything past the source window but one card-sized, drop-centred box sits under the scrim, and that box holds a tinted border several times what the same box held before. `views/root.rs`
  drives keys. The equaliser's note is always drawn and `kit::group_heading` holds a row control's
  height, so pressing the curve with the equaliser off moves nothing under the press.

## Panes

**Listening Party** (`views/party.rs`) and the sharing sheet (`views/sharing.rs`) read through the `Together` seam,
absent where the build or setting has no sync, so neither sees the sync crate. Both redraw on a timer
while shown, reading in-memory state only; the sheet asks twice for what cannot be undone (`party.md`,
`shared-playlists.md`).

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
- **Resonate Wrapped is a sheet over the pane, offered from the Statistics heading and never unasked.** `views/wrapped.rs`: `open_wrapped` reads on the background executor and sets `RootView::wrapped`, a `Deck` (the years heard, the review, its slides, the slide shown); `Sheet::Wrapped` keeps the focus, escape closes it, and the plain arrows, which are the seek keys, turn the slide (`turn_the_slide`) because a sheet in front holds them back from the transport. A slide fades in over `HANDS_OVER`, keyed by its place; there is no auto-advance. **The deck is centred over the scrim at one height whatever the slide** (`kit::over_a_scrim`): a stage of `theme::wrapped_stage()` (380, the tallest slide, five ranked rows under a headline and detail) holds each slide centred in it, so the card and its controls stay put as the slides turn and none floats in an empty box; past the window's `DECK_HEIGHT_AT_MOST` the stage gives way and the slide scrolls. The card is the plain sheet, no wash or gradient under it. The head is a progress segment a slide, lit to the one shown (spoken as *n of m*, `slide_of(at, count)`), the figure and a close mark; a `kit::rule` and the foot close it: the years, *Copy* and *Save card*, a hairline, then *Back* and *Next* (primary) together, *Next* becoming *Done* on the last slide. A slide with no rows or bars is centred and draws its headline at `text_lyric`; the eyebrow is in the accent, and the first ranked row is washed in it. The opening slide draws the year's plays, songs, artists, albums and days as one `kit::ledger` of five cells (`tallies_drawn`) in place of its detail line. Each row and bar is a list item saying what it shows. *Copy* takes `YearReview::summary`; *Save card* writes `wrapped_card.rs`'s picture, an SVG written by hand in the worn palette and drawn by `resvg` on the background executor with the system's fonts (`the_arrows_turn_the_slides_and_escape_closes_the_deck_leaving_the_pane_alone`, `a_name_written_on_the_card_cannot_break_out_of_the_drawing`).
- **The Statistics pane reads top down from one figure to the rankings** (`views/statistics.rs`):
  *Time listened* and its pace over every day the window spans (`the_pace_of`) set on the page at
  `text_lyric`, no card and no gradient under it, beside four counts; the number ones with their
  art, the chart, the moments (busiest day, longest run, the hour and the day heard most), when in
  the day and the week, genres beside the sound, tracks, then albums beside artists. **Figures that
  belong together are one `kit::ledger`, never a tile each**: the counts, the number ones and the
  moments are each one bordered surface (`kit::section`'s shape) cut into equal cells by hairlines,
  `across` cells a line (four, three and four when wide; two, one and two below), a cell an eyebrow
  over its figure with no icon in a tinted square. **One colour, one meaning holds in a chart**:
  every meter and bar is the accent (`a_fill`), the first or the peak whole and the rest worn
  thinner; a genre is not given a hue of its own. **Two columns stand only from `statistics_two_columns()`**, read off the `grid_width` a
  `kit::measures_the_grid` under the pane writes, not a wrapping row (taffy sizes a wrapping flex item
  at min-content width); below it everything stacks. A ranked row is a `tall_row`: rank, art, name over
  its by-line (the maker and plays), then a meter `theme::row_plays()` wide filled to its share of
  the first row and the time heard right-aligned in `theme::row_length()`, so the shares line up as
  a column instead of a wash of a different length behind every name. The three lists stay one reach run in
  order (`heard_rows`).
- **The statistics chart is `div`s, not a canvas** (a few dozen discrete bars; a `div` carries a
  `hint::Names` hover free). `a_standing_bar` is every bar of the three charts, the peak lit, the rest
  the accent worn thinner. The header says the average, drawn as a line, and the peak, and how many
  days a folded bar holds, since a peak over four days is not a day's. A day with no plays still draws
  its baseline; a run over `BARS_AT_MOST` (120) folds whole days into one bar. Axis: *today*, *yesterday*, else the date via
  `resonate-core::CivilDate` of the day `Calendar::local` puts each bar's midnight on (the calendar
  history buckets plays by); `Chart::in_calendar` takes one so tests hold the axis to UTC and a zone
  east of it (`a_day_is_dated_in_the_listeners_calendar_rather_than_greenwichs`).
- **Suggestions: shelves by kind; a card opens onto what it would hold.** `SuggestionKind::ALL` groups
  cards under *Made for you*, *Rediscover*, *From your listening*, *Eras*, *Genres*, *Artists*, *Sound*, each a `kit::group_title` over a
  wrapping row. **A card is a cover cell, not a boxed card with buttons**: the mosaic at
  `grid_side()`, then the name (lit under the pointer, `pointed.rs`), the reason on one line and the
  count and length in `faint`, as an album or playlist cell reads; *Play* is `kit::play_on_the_art`
  (the round accent mark a playlist cell raises too) and the queue and save marks stand on a
  `kit::cover_veil` in the art's corner, all three shown under the pointer and each stopping the
  press, so a shelf of twenty lists holds no row of primary buttons. Pressing art or name = `LibraryModel::open_suggestion`: `SavedQuery` kept in a
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
  the one builder: `Mosaic` = covers drawn, a mark, a name, rounded all round. `suggestion_art` draws covers in
  `Suggestion::pictured_by` (one whole, or two to four as a 2×2 mosaic). **Where no cover is, the
  ground is neutral**: art with no picture at all is `raised` with its mark in `faint` and its name
  in `muted`, and a tile a mosaic lacks is `raised` or `hover` by turns, as an album without a cover
  is a neutral disc (`kit::uncovered_mark`); no gradient and no accent pair is drawn (the phones
  still name a pair through `resonate_core::Accent::named_pair`). Side = whole pixel, near tile the floor of
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
  alone. A lone word draws no chip. A pane with nothing left says *No albums match* where a search is
  in force, *No albums yet* where none is, and *The library couldn't be read* (`kit::trouble`, with a
  toast) where the last read failed (`LibraryModel::unreadable`, checked before the narrowed wording). A failed read for a query other than the one the lists were read for empties them (`listed_for`, `forget_the_listing`); a failed refresh of the same query keeps its rows.
- **The window keeps every zone's queue for the next run; the switch stopping it discards what was
  kept.** Each `ZoneShown` holds a `Keeping` and a `Listening` off its own `PlayerModel` observer
  (`RootView::zone_moved`), keeping what `resonate play` keeps (`audio.md`) under its `ZoneId`, and
  its `KeptZone` (name, device, volume) where it moved (`keep_the_zone`). Every zone's plays count. `Stored::resume` is a `config.toml` key; the *Resuming* card is a
  Library group (it writes to the catalog). Off = `Library::forget_resumption` at once. Each write
  awaits the one before it (`keep_after_what_was_kept`).
- **Tab, restore size and Settings category each have a memory switch** (`remember-tab`,
  `remember-window-size`, `remember-settings-category`, default on, via `Stored` into `ResonateApp`).
  The tab is the pane `in_front` names; a tab hidden by `Tabs` is ignored on the next open. The size is
  written after `WINDOW_SIZE_SETTLE` (400 ms) so a drag edits the config once. Switching off removes the
  saved value.
- **A newer release is told once and offered in About.** `ResonateApp::updates` is the binary's
  `Updates` seam (`None` where the build names no repository or lacks `online`);
  `RootView::look_for_a_newer_release` runs from `RootView::new` and again when *Check for updates*
  is switched on, asking `Updates::newer(reaching)` on the background executor with the live Online
  switch every `LOOKED_AT_AGAIN_EVERY` (6 h; the binary keeps it to once a day). A release other
  than the one `update_dismissed` names lands in `RootView::newer` and is toasted once (`Noted`,
  *Resonate X is out · Settings, About has it*); the About category's *Updates* group
  (`Group::Updates`, keys `check-for-updates` and `update-dismissed`) holds the switch and, while
  one is known, *Open the release* (`cx.open_url`) and *Dismiss*, which writes `update-dismissed`
  (`a_newer_release_is_told_once_offered_in_about_and_dismissed_for_good`,
  `switching_the_check_off_forgets_the_release_and_asks_no_more`).
- **The window is handed its lookups and owns none.** `Lookups` is what `run` takes: `lyricists`,
  `fingerprinters`, `reference` (`Option<Arc<dyn Reference>>`), `for_the_pass` (`Consulted`),
  `scrobblers`, `signs_in`, `spotify`, `corrections`, the `online` setting, `bindings`, `measures` (the sweep of the Equaliser pane's *Measure*, `eq.md`), `sourcing`, `serving`, `listens`;
  `ResonateApp` holds them as globals, so `LibraryModel::new` knows whether it `can_enrich`. A scan not
  cancelled runs `enrich(false)` as it ends where `can_enrich` (online *and* a reference) allows; the
  lookup polls at `SCAN_POLL` and reloads every `POLLS_PER_RELOAD` polls. Online off under a run keeps
  the reference and only stops `enrich` starting (`has_reference`); the shared client hears the switch
  at once (`online.md`); `AnalysisModel::reach` stops recognition asks while off.
- **A file changed under a root while the window runs is followed on its own; one taken away is
  forgotten at once, not scanned for.** `resonate_library::RootsWatch` = recursive watch (inotify,
  FSEvents on a Mac, via `notify`):
  - *Gone* (audio file or folder removed): handed out by `taken_away` once `GONE_QUIET_FOR` (250 ms)
    has passed, never while a change under the roots is settling (a rename is a path gone and changed
    at once; the scan follows the file instead of forgetting and recounting it). **FSEvents names
    neither side of a rename and folds a file's flags together**, so an event is weighed against the
    disc: a rename (`RenameMode::Any`) naming no path that still stands is the side taken away, and a
    making or writing naming none says nothing, so a file written and removed within one FSEvents
    latency does not hold its removal back as a change
    (`a_rename_that_names_neither_side_and_a_making_since_undone_are_weighed_against_the_disc`).
  - *Changed* (audio file or sheet made, written or renamed in; a folder made or renamed in; a sheet
    taken away): `settled` hands the root out once quiet for `ROOTS_QUIET_FOR` (2 s), or once its first
    change has waited `DEFERRED_AT_MOST` (5 minutes)
    (`a_folder_that_never_goes_quiet_is_handed_out_once_it_has_been_deferred_long_enough`). An
    overflowed inotify queue notes every root. A catalog inside a root cannot trigger the scan writing
    it; a vault inside one does, and the scan steps past it.
  - `watch_the_roots` looks every `ROOTS_LOOKED_AT_EVERY` (1 s) at what the watch heard, and reads
    `Library::roots` and stats each (`Watching::following`) only every `ROOTS_READ_EVERY` (5 s)
    (`the_roots_are_read_once_in_a_while_and_not_at_every_look`), rebuilding the watch wherever
    they moved, over roots present as directories (`RootsWatch::replacing` drops the old watch first,
    so two never hold inotify watches at once, and carries what it heard and had not handed out; one
    that cannot be laid is tried again at the next read). What is gone goes to `Library::forget_the_gone` (takes the `Walk` guard, deletes rows
    whose file is missing, sparing a vaulted and a delivered row as the scan's prune does, and rows on
    an absent volume); a forget refused by `Error::AlreadyWalking` or failed is kept and retried. A
    settled root with the work slot free runs the incremental scan, `Prompted::OnItsOwn`. A non-UTF-8
    path is passed over, not failing its batch.
  - A watch that cannot be made is a warning; one that cannot cover a root
    (`RootsWatch::leaves_a_root_uncovered`) is laid again on the same watcher every
    `UNCOVERED_ROOTS_TRIED_AGAIN_AFTER` (5 minutes, `RootsWatch::cover_again`,
    `a_root_that_appears_after_the_watch_was_laid_is_covered_on_the_same_watch`); proved past the limit by
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
  `words::transport::cover_hint` and `RootView::magnify` (a portrait does not: `Magnified` is an album or a file).
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
    unread*) under `words::browser::unheld_hint` opens `Pane::Missing`; with unread releases and `can_enrich`, *Read the
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
  link to an album not held; `fetching_album` reads its songs from `Downloads` for the colour and
  `told_album` says them as the sidebar does (`downloads::summed_up`: *Downloading… · 9 left* while any song's
  bytes arrive, *Searching… · N left*, then *N downloaded · N not downloaded*), never one song's state for the
  whole album. *Tracks* follows the
  held rows with `ListedRow::NotHeld` (*Not in your library · N songs · press one to download it*) and
  a `found_row` per song (press = `want_found`) via `restate_the_listing`/`beyond_the_listing`, which
  answers nothing until the listing is whole; a page query narrows them via `still_answering`.
- **The server pane browses whichever services play in place and is there only while one is set.**
  `Pane::Server` (`Icon::Server`, key `server`, labelled *Services* since YouTube Music is no server)
  is the last row of the Collection section, in `Pane::BROWSE` after `Missing`, and `Pane::is_there`
  is what the sidebar, `stepped_pane` and `set_pane` ask: it weighs `ServerModel::is_open` (a service
  the live `Online` sets, online on) beside `Tabs`, so with no service there is no row, the pane keys
  pass from the collection to what is playing and `set_pane` lands on the first tab, and a service let
  go takes the row and the pane with it. A press is `choose_pane(Pane::Server)`. `Tabs` does not hide
  the row: that would be a stored setting of its own. The remembered tab never lands on it at start,
  because a first page reads the listener's account and `ServerModel::shown` reads it only once the
  pane is shown. The window reaches the services through the
  `Serving` seam on `Lookups` (`ResonateApp::serving`): a `Fn(&Online) -> Vec<Arc<dyn Serves>>` the
  binary fills (`providers.md`), each `Serves` naming its `scheme` and `title`; `Serves` is gpui-free vocabulary
  (`served.rs`: `ServedArtist`, `ServedAlbum`, `ServedSong`, `ServedFound`, `ServedCover`, every
  call a blocking `resonate_providers::Result`), so the window names no provider crate's types.
  `ServerModel` (`serving.rs`) is an entity held by `RootView`: a trail of `Place`s (`Artists`,
  `Artist`, `Album`, `Found`), the `Page` of the place in front (`Reading`, `Read` rows, `Failed` with a
  `Failure`: the account turned away, the server not reached, an answer that did not read, each
  with what to check) read on the background executor and replaced if a newer read began, and
  `refresh`, which `observe_global::<ResonateApp>` calls on every change of the global and which
  does nothing unless the services moved (the same `Arc`s in the same order); moved services start
  at the artists again on the service chosen before where it is still set, else the first, and
  forget the covers. **With several services a chooser heads the pane**: a `kit::tab_strip` of
  `kit::tab`s, their titles (`served-service-<n>`), up to `SEGMENTS_AT_MOST` (3), else a `kit::pick` (`served-service`) of the
  chosen title opening a `Menu` of them (`services_menu`, `more_services_than_fit_a_bar_are_chosen_from_a_menu`), both
  ending in `RootView::choose_a_service` (`ServerModel::choose`), the first page drawing no
  eyebrow under the tabs (the chosen one's title where there is one service and no tabs); a choice starts at its artists and clears the search. **The first page is
  `Serves::first_page`** (`artists` by default; the binary's adapter hands the service's own,
  `providers.md`): artists alone are listed bare, anything more under the search's headings
  (`serving::first_rows`) and titled *Favourites*, counted as a search is
  (`a_service_with_favourites_opens_on_them_and_a_favourite_song_plays_from_the_first_page`); a
  service's favourite mixes are a *MIXES* section of `ServedFound::mixes` (`Row::Mix`, `Place::Mix`:
  *MIX ON THE SERVER*, opened to its songs as an album is, counted in the first page's line,
  `a_mix_on_the_first_page_opens_to_its_songs_and_a_song_of_it_plays`). **The star is the service's
  own mark** (`Serves::keeps_favourites`, `Serves::favour`; `kit::star`, so it fills in the accent):
  a song is a favourite where the model's `favoured` note, kept for the run and emptied when the
  services move, says so, else where the service said `ServedSong::favourite`, else not. A press
  notes the new state at once, writes it on the background executor
  (`RootView::favour_a_served_song`) and, on a refusal, puts the note back and toasts the
  `Failure` (`a_star_on_a_served_song_favours_it_in_the_service_and_a_favourite_one_is_taken_out`). A service that lists nothing
  unasked opens on *Search to begin* (*This service lists nothing until it is asked*) and the search
  (`two_services_are_chosen_between_in_the_pane_and_a_song_found_on_the_second_plays_from_it`). The pane (`views/served.rs`) is a heading (way back, title, the count, *Play* and
  *Add to queue* on an artist or album, and a search `Field` that asks the server on enter and an
  empty enter leaves the search) over one `uniform_list` of rows; a row is a cover, a title with its
  line under it, a figure, and three marks (play, play next, add to queue), a song of a service that
  `keeps_favourites` with a star before them. A press opens an artist, album or mix, or plays a song: on an album page the whole album from that song (`songs_from`), in a
  search the song alone. An album or artist played or queued is expanded to its songs on the
  background executor (`Serves::expand`, a failure a toast naming the `Failure`), and rows are
  `QueueItem::whole` with ids minted beside the queue's (`play_served`, `queue_served`). The server
  is only ever asked for what the pane shows: nothing is read until the pane is opened
  (`a_server_is_a_pane_only_while_an_account_is_given_and_online_is_on`,
  `an_artist_opens_to_its_albums_and_an_album_to_its_songs_and_a_song_plays_the_album_from_it`).
- **A served song's cover is its service's.** A cover cell for a row whose location any service
  names (`ServerModel::names`) draws `ServerModel::cover` from the service that names it (keyed by
  scheme and cover name), not `Player::art`, in `drawn_cover` and
  `is_pictureless`, so the queue, the playback bar and the pane draw one picture: a row's own
  `ServedCover` where the browse gave one, else the song's (the model asks `Serves::cover` with no
  name and the adapter asks the server what a song's is), fetched on the background executor
  `COVERS_FETCHED_AT_ONCE` (4) at a time, decoded on the `Drawer` at the drawn side, held in a
  `Recent` of `COVERS_HELD` (256) keyed by name and side, a failed one asked again after a minute.
  The magnifier opens `Magnified::Served` at the now-playing size.
- **YouTube Music is a group in the Accounts category, and what it holds is kept, not played.**
  `Group::YoutubeMusic` (`views/settings/youtube_music.rs`): the switch, the cookies and `yt-dlp`
  fields, the keep-order switch, the terms in one plain sentence beside them and *Check connection*,
  which asks the `ChecksYoutubeMusic` seam (`Lookups::checks_youtube_music`) on the background executor
  and toasts what it heard. **It signs in through the sign-in window**: the `SignsInToYoutubeMusic`
  seam (`Lookups::signs_in_to_youtube_music`, the binary's `signing_in::FromTheWindow`), where it
  `is_offered` (the program found), leads the action row with *Sign in to YouTube Music*
  (`youtube-music-sign-in`; *Sign in again* where `names_the_window_jar`, *Signing in…* while it
  runs on the background executor), greyed like the check; `YoutubeMusicSignIn::SignedIn` puts the
  jar in the cookies field, the global and the file, moves the sources and runs the check;
  `Closed` is a note that nothing was kept, the setting untouched. *Sign out*
  (`youtube-music-sign-out`) stands only beside the window's own jar and clears it. Not offered, the
  group says to install `resonate-signin` and draws no sign-in
  (`signing_in_to_youtube_music_names_the_jar_and_checks_the_account`,
  `a_sign_in_closed_keeps_the_browser_the_settings_named`,
  `sign_out_is_offered_only_for_what_the_window_kept_and_clears_it`,
  `without_the_sign_in_program_the_pane_offers_no_sign_in`). In the server pane a service that `Serves::keeps_songs` draws a *Keep* mark
  on a song row (`server-row-keep-<n>`) and *Keep* on a mix's heading (`server-keep`, once its rows are
  read, the name on the row it was opened from carried in `Place::Mix`), and no *Play* where
  `Serves::plays_songs` says no. A press is `LibraryModel::keep` or `keep_mix`, its pass followed through
  the sidebar's *Downloading… · N left* (`downloads::keeping_said`, `keeps_left` read each second) and a
  *Keeping songs* row with *Stop* at the head of the *Downloads* panel, a toast telling what came of it;
  no music folder is a toast saying to choose one (`providers.md`'s *Keeping a song from a service*;
  `a_service_that_keeps_songs_offers_a_keep_on_a_song_and_a_mix_and_a_press_queues_them`).
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
  `Icon::Missing` (*Nothing missing matches* while narrowed).
- **A song asked for is followed in the sidebar, never told in a toast.** `downloads.rs`: `Downloads`
  holds a `Download` per found song pressed (the `Found`, `WantId` once landed, when queued, a
  `Fetching`); `LibraryModel::want_found` writes it: `Landing`, then `Queued` (`NoProvider` where
  `Sourcing::providers` registers none, `Unwanted` where landing failed). **The providers are asked
  while the release is read**: `want_found` runs `Library::foresee` on the background executor beside
  `want_found_uncovered` and starts the poll once both are done, the want reading *Waiting its turn…*
  meanwhile (`providers.md`; `a_found_song_pressed_lands_once_as_the_held_track_and_plays` holds that
  the one ask began before the release was read). `Downloads::followed` reads
  each shelves load's `WantStanding`s: delivered = `Downloaded`; tried since queued with nothing offered
  = `Retrying { tries, at }`; given up = `GaveUp`. **A download says which step it is on.** `Landing` reads *Reading the
  release…*, `Queued` *Waiting its turn…*, a `Downloading` *Searching…* (*Searching <provider>…* once
  the poll names the provider asked, `PollProgress::provider_of`), *Starting the download…* once an
  offer is being opened (`PollProgress::is_opening`, set by `opens_from` as `offer_landed` takes it)
  and *Downloading…* once `Fetched::is_arriving` (`downloads::saying_while`, `LibraryModel::fetched`),
  on the panel and the song's own row (`told_found`, `told_want`); `Retrying` reads *Not found yet ·
  next try HH:MM* on the listener's clock (`Calendar::minute_of_the_day`), *Not found yet · trying
  again* once due; `Unreached` *Can't reach the providers · trying again*. Never an attempt count.
  Sidebar `summed_up` says *Searching…* or *Downloading…* with *· N left* while more than one song is
  underway, else *N downloaded · N not downloaded*
  (`a_download_says_which_step_it_is_on_from_the_release_to_the_bytes`,
  `a_song_not_found_yet_says_when_it_is_next_tried_on_the_listeners_clock`,
  `the_sidebar_says_searching_or_downloading_and_how_many_songs_are_left`). Only an ending speaks
  otherwise: *Downloaded*, *Not found*, *No provider is set up*, *Couldn't add it*. A poll ending with a
  provider refusing or late (`PollStats::refused`/`late`) sets `LibraryModel::providers_unheard`; while
  set, a queued or past-due retrying download reads `Unreached { attempt }` (`providers.md`;
  `Fetching::while_polling` is the one reading). **Providers that keep refusing are asked less often**: each self-started
  poll ending unheard with nothing kept doubles the timer's floor (`unheard_in_a_row`,
  `retried_after`: `RETRIES_ASKED_AT_LEAST` times two per poll, at most
  `UNHEARD_RETRIES_DOUBLE_AT_MOST` doublings), one heard resets it; a press still asks at once
  (`providers_that_keep_refusing_are_asked_less_and_less_often`). **Standings are re-read while the poll runs**
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
- **A song is matched to a recording in a sheet of its own** (`views/matching.rs`). *Identify ›
  Match to a recording…* on a track's menu opens `matching_sheet` (`Sheet::Typing`,
  `MATCHING_PANEL`) on a `Matching` for the track: the `typed_words` field opens holding the song's
  title and is asked at once, again on enter or *Search*, through `LibraryModel::recordings_like`
  (`Library::recordings_like`, a near search the strict rule does not weigh), a later answer
  replacing an earlier only where it is the last asked (`Likely`: asking, found, unreached, Online
  off). Each recording found is a row (title, credit, length, the release it is on,
  `likely-recording-<n>`); a press is `LibraryModel::take_recording` and closes the sheet. Below,
  *Or give it an ISRC*: the `typed_isrc` field, checked by `Isrc::new` on enter or *Give it*, a
  refusal a toast and the sheet kept open, a landed code starting a lookup that asks the track first
  (`give_an_isrc`). Escape, a press outside and *Cancel* leave it
  (`a_song_matched_to_a_recording_its_name_only_nearly_spells_takes_that_recording`).
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
    it (`a_found_song_that_landed_is_listed_once_as_the_held_track`); until then, delivered or not, it
    stays listed as found
    (`a_pressed_song_no_provider_delivers_is_still_found_when_the_search_is_asked_again`). **A found song's whole row is that
    press** (`found_row`, `words::browser::fetch_found_hint`); the mark's own press finds the recording already
    `wanting`; while asked for, the row takes no press. **A right press on a found song offers it without playing
    it** (`found_menu`): *Download and play* (the row's press), *Download* (`want_found` alone, nothing awaited)
    and *Download from a release* nesting the releases the heart's right press lists
    (`a_right_press_on_a_song_found_on_musicbrainz_downloads_it_without_waiting_to_play_it`). `wanting` holds a task per recording.
    **A song pressed plays once it lands, if it lands within half a minute.** The row's press, `enter`
    on a found row and on *Top results* call `LibraryModel::fetch_and_play`: `want_found`, then
    `Downloads::play_once_landed` (one song awaited, the latest press winning; the heart, a release
    from its menu, a pasted link and *Ask again* only want). `take_the_wants` asks
    `Downloads::landed_to_play`, which answers the held `TrackId` once the want is delivered and
    scanned within `PLAYED_IF_LANDED_WITHIN` (30 s) of the press and lets go of a song that took longer,
    gave up or was dismissed; `RootView`'s observer drains `take_arrival` into
    `play_what_was_downloaded`
    (`a_song_pressed_plays_once_it_lands_unless_it_took_longer_than_half_a_minute`,
    `a_found_song_pressed_lands_once_as_the_held_track_and_plays`). While underway the row's cover
    wears `fetching_over`: `kit::cover_veil()` and `Icon::Spinner` turning in the
    accent (`motion.md`).
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
      (`songs_kept_from_a_discography_come_first_and_musicbrainz_does_not_repeat_them`); **a kept
      song no code names gives way, in its own place, to MB's take of the same title and credit
      that carries one**, since a provider searches by code first and a kept song carries none
      (`a_kept_song_no_code_names_gives_way_in_its_place_to_musicbrainzs_take_that_carries_one`). Albums likewise
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
  Deezer, ListenBrainz, Spotify, TIDAL, Apple Music or YouTube playlist link fills a playlist of what is held
  and wants the rest**: `followed_a_playlist` wants every found song, then `Library::fill_playlist_named`
  on bg puts the held songs into the playlist of that name (made where none is; an existing one gains
  only what it lacks), opens it and toasts (`playlist_told`); where nothing is held no playlist is made
  (made through `Library::fill_from_the_link` with `Upkeep::Once`, so the playlist remembers its
  link); **a pasted Deezer profile link is followed whole, in step** (`followed_a_profile`:
  `Library::follow_profile` on the background executor, every found song wanted, the Playlists pane
  opened, `profile_told` counting the playlists and the songs downloading; a private profile or one
  with no playlist is a `Trouble`), and `refreshed_where_due` follows each profile
  `profiles_due_a_refresh` names before the playlists due; **an opened playlist with a link says where it came from and how it follows it** (*imported
  from Spotify* / *kept in step with Spotify* in the line under its name) and carries *Keep in step
  with …* / *In step with …* (`LibraryModel::keep_playlist_from_its_link`, an edit) and a refresh
  mark (`LibraryModel::refresh_playlist`, toasted). `keep_the_linked_in_step` looks a minute after
  opening and every `LINKS_LOOKED_AT_EVERY` (10 min) while Online is on and reads every playlist
  `playlists_due_a_refresh` names; found songs go through `want_found` (the downloads list) and a
  playlist the read moved is toasted, a quiet one is not
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
  the frame twice). The row cap rides beside the name as segments and the order as a `kit::pick` opening the
  Sort dialogue on `Ordered::SavedSearch` (a saved query = search, order, row cap). *Edit search* = `RootView::revise_search`. The Online card's fields follow one pattern:
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
  (`Shift`, anchor, the row moved to, and for the catalog listings the rows chosen apart; lives for the run, and a non-queue reach is let go of when the
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
- **The catalog listings choose rows apart.** Only `Shift::chooses_apart` (Tracks, Albums, Artists,
  the playlists index, Favourites, the Favourites shelves, an opened suggestion, an opened playlist
  and the songs of Top results; not the queue, Missing or the Suggestions and Statistics shelves,
  whose runs mix kinds of row) takes `ctrl`-click (add the row, or let it go if chosen) and `shift`-click (widen from the
  anchor, keeping what was chosen apart); a plain click still plays or opens, a modified click only
  chooses (`RootView::reach_by_click`). `ctrl-space` chooses or lets go of the row the cursor is on
  (`ChooseReached`), `ctrl-shift-up`/`down` hop the cursor a row without touching the choice
  (`HopAbove`/`HopBelow`; `ctrl-up`/`down` are the volume). A `Reach` is the run `anchor..row` plus
  `apart: Chosen` (sorted, merged `Span`s) and `chosen`, whether the run is itself chosen: a cursor on
  an unchosen row is `chosen: false`, drawn as a faint left edge (`reorder::cursor_marked`) or a faint
  ring (`cursor_ring`). `Reach::selected` is the whole choice, `toggled` and `hopped_to` the two moves;
  every other gesture builds `Reach::at`/`spanning` and so drops the rows apart
  (`an_arrow_gives_up_the_rows_chosen_apart`). **Every action on a row in the choice acts on all of
  it** (`RootView::acting_on_chosen`: the choice if it holds the row, else the row): `enter` plays the
  chosen tracks in order or queues the chosen albums' or artists' tracks
  (`RootView::play_the_chosen`; `plays_everything_in_each`, one background read in order), the row menu
  becomes `chosen_tracks_menu`/`chosen_albums_menu`/`chosen_artists_menu`/`chosen_playlists_menu` (Play, Play next, Add to
  queue, Add to playlist where tracks, Favourite or Unfavourite all where albums, artists and tracks; playlists
  queue the songs of each in turn through `RootView::queue_the_playlists`, one background read), a track's two queue buttons queue
  the choice (`queue_chosen_control`), and a drag lifts it. **One row of code serves every track
  listing**: a track row carries a `Rowed::chooses` (`browser::Chooses`: the `Shift` and the listed
  row; none where the rows are not a choosable listing), and `RootView::tracks_chosen` turns a `Chosen`
  into tracks for Tracks (`tracks_at`), Favourites and a suggestion (the page's own rows);
  `RootView::marked_where_reached` draws the reach edge and the cursor's faint edge for every one. A
  choice of several tracks also offers *Hide from library* (or *Show in library* when all are
  hidden: `LibraryModel::hide_tracks`, one edit), *Delete from disk…* (one `Deleting::each` sheet and
  one `delete_tracks` for the lot, a toast counting them) and *Copy the file paths* (one per line,
  local files only), *Show in the file manager* (`menu::one_file_in_each_folder`: one `reveal_path`
  per folder, on the first of its files, since the portal reveals one file and a folder opens once) and
  *Share each* (`LibraryModel::share_each`: every link on the clipboard one to a line, the toast
  counting them and saying how many of the choice had one). What takes one row still: *Go to* entries
  and the other copies; album and artist menus never had Hide, Delete or file entries.
  **Top results chooses its songs only.** The run mixes songs, found songs and strips of artists and
  albums, so `RootView::choosable_rows` is the songs shown (`TopRun::songs`): `reach_by_click`,
  `choose_the_reached` and `hop_to_a_row` refuse a row past it (a strip keeps its plain click, found
  songs theirs), and `RootView::choosable` clips a reach that arrow keys or *reach every row* ran
  across kinds to its songs wherever a choice acts (`enter`, the menu, the queue buttons, the lift,
  `tracks_chosen`). **An opened playlist chooses its entries apart too, and its edits take the
  choice whole**: *Take out* (`delete`, the ✕, the menu) is `RootView::drop_chosen`, one
  `remove_runs_from_playlist` and one undo step, leaving the reach on the row where the first run
  stood (`Reach::after_dropping_runs`); a drag, `alt`-up/down and the row arrows are
  `RootView::shift_chosen`, which for one span is the old `Command::Move`/`move_in_playlist` and for
  several is `gather_in_playlist`, the rows setting down together in order at the drop row (arrows:
  one place above the first chosen row or below the last) and the reach becoming that block
  (`Chosen::gathered_by`); `Carried` carries a `Chosen`. `enter` on a choice plays the playlist from
  its first chosen row (`Shift::plays_the_chosen_rows` is false there: the badge and the queue
  stamp belong to the whole playlist), while the queue buttons and *Add to playlist* take exactly
  the chosen entries (`Reaching` holds a `Chosen`). **The Favourites shelves join the pane's reach.**
  Each strip is a `Listed` of its own (`FavouriteArtists`, `FavouriteAlbums`; the cell index in the
  strip is the row), so `ctrl`-click, `shift`-click and `ctrl-space` choose and widen along one strip
  and a choice never spans two, and the cell's menu, right-pressed or by the menu key (an artist cell
  is `opens_a_reachable_menu` as an album's is), becomes `chosen_artists_menu`/`chosen_albums_menu`
  over `RootView::artists_chosen`/`albums_chosen` (the shelf's own list, never the Artists or Albums
  listing: a narrowing search leaves those holding other rows), with Unfavourite all in place of
  Favourite all; an album cell lifts the choice as a drag does. `favourites::Shelved` is the
  arithmetic, the Favourites pane's three sections in drawn order (artists strip, albums strip,
  songs), each a `Shift` of its own, since the songs keep their row numbers: `reachable` answers the
  section the reach stands in, else the songs, else the nearest shelf holding cells (so a first
  arrow lands on the first song as it always did). `up` leaves the first song for the albums strip,
  and a strip for the strip above it, `down` a strip for the next section, a section with nothing
  in it passed over (`RootView::reach_across_the_favourites`); `left`/`right` step the cells of a
  strip as Top results' do (`reach_along_the_strip`, one arithmetic for both) and seek on the songs;
  `shift-up`/`down` widen and `ctrl-shift-up`/`down` hop along a strip's cells, and `home`, `end`,
  the page keys and `ctrl-a` act within the section reached. `enter` opens the one cell reached
  (artist or album, as a click) or queues the songs of every cell chosen in turn
  (`play_the_chosen`). A reached cell slides into view along its shelf
  (`kit::brought_into_view_within` with no vertical handle, `reached_unseen` set by `show_row`), the
  pane itself never scrolling. A change in how many artists or albums are favourites lets go of any
  reach (`Arrangement::shelves`), since the cells under a stale row would otherwise wear it
  (`the_arrows_walk_the_favourites_from_the_tracks_up_into_the_shelves_and_back_down`,
  `ctrl_space_chooses_favourite_artists_apart_and_enter_queues_each_in_turn`,
  `the_menu_key_on_favourite_albums_chosen_apart_plays_each_in_turn`,
  `unfavouriting_the_artists_chosen_on_the_shelf_lets_go_of_the_choice`).
- **A track or album is dragged out of a listing into the queue or a playlist.** Track rows and album
  cells carry a `reorder::Lifted` (`Lift::Tracks`/`Lift::Album`/`Lift::Albums`) through
  `reorder::liftable`; a row standing in the choice carries every loaded row of it as a
  `reorder::Run` (or `AlbumRun`): the page's own `Arc<[Track]>` and a `Chosen`, never a copy, so a 2 000-row selection costs a row an `Arc` clone per
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

- **Settings pane: a rail of categories beside a centred column of boxed groups; one table says what
  a group is and where it is drawn.** `views/settings/` is a folder. `find.rs` = vocabulary:
  `Category` (Output, Processing, Equaliser, Playback, Library, Filters, Online, Recognition,
  Accounts, Sync, Desktop, Appearance, Window, About), `Page` (a page inside a category) and `Group`,
  one per setting, whose one match `Group::described` gives its category, its page (`None` for the
  category's own page), title, hint, *also*-called words and the `SettingKey`s putting it back; every
  other accessor reads that row. Nothing else enumerates groups: `Category::groups` filters the one
  table, `Category::shown_on(page)` a page of it, `Category::drawn` the category's own page in table
  order (`Drawn::Group`, and `Drawn::Pages` where the first group of a page stands, consecutive pages
  sharing a `Page::gathered_under` gathered into one card), `mod.rs::group` dispatches a `Group` to its
  drawing method. Tests hold every group to one category, a unique title, its own hint, no key claimed
  twice, every page to groups of its own category, every group drawn once and every category to a
  mark of its own. Rail = `theme::settings_rail()`, a tab list a reader hears (`Role::TabList`, each
  category a `Role::Tab`, selected where open), its rows the sidebar's own (`theme::sidebar_row()`
  tall, `rounded_md`, the label ending in an ellipsis); `RootView` remembers the open category for
  the run. A filter that finds nothing is a `kit::empty`.
  Body = a column `theme::settings_column()` (720) wide at most, centred in the pane with a measure of
  padding on every side, of `kit::group`s a measure apart, then *Reset <category>* right-aligned at
  its end. Body scrolls under `RootView::settings_scroll`; `show_settings` resets it on category or
  page change (gpui keys scroll offset by element id).
- **A group is a title above a card of rows.** `kit::group(heading, rows)`: `kit::group_heading`
  holds `kit::group_title` (`text_sm`, semibold, in `text`, a level-2 heading), the *Experimental*
  badge, the revert mark and the info mark; under it a `kit::Rows` (`kit::rows()`), a card on
  `surface` with a `soft_edge` border whose every child is a row, laid in a padded cell with a
  `soft_edge` hairline before each but the first (`Rows` is a `ParentElement`, so `.child` is the
  whole API), then the group's `note`s under the card (`Rows::note`), at the rows' inset. **One label
  style**: `kit::row_name` (`text_sm`, medium, `text`) over a `muted` `text_xs` line under it
  (`kit::named`, or a `note` restyled where it must be read plainly); `kit::row` puts a name and its
  line on the left and a control on the right (at most `ROW_CONTROL_SHARE` of the row),
  `kit::stacked_row` and `kit::field` put a wide control (a long segmented choice, a field, a list, a
  grid) full width under them, `reading` a figure on the right. A pressable row
  (`kit::switch_row`, `kit::nav_row`, `kit::choice_row`, a folder) **bleeds** to its cell's edges
  (`kit::Bleeds`: margins as deep as the cell's padding, the same inside, rounded), so its hover
  wash, chosen wash and focus ring fill the cell. The card's own padding keeps a wash 4 px off its
  edge, and the hairline between cells (`row_rule`) stands 4 px clear of the cells either side and
  as wide as a wash, so a lit row sits evenly inside the card and lines up with the rule beside it. A row
  of buttons is a `kit::action_row`. **One framed text field**: `kit::framed_field(id, lit)` (the
  search height, `background`, a `soft_edge` border lit in the accent under the pointer or while it
  holds the caret) is every field's frame; `RootView::text_field(id, field, leave)` is every
  settings field: a press focuses the `Field`, a press outside calls `leave` where the field held the
  caret, and the faint *enter* says how a value is given. The settings filter alone draws the frame
  with its search mark, *ctrl-,* and the clear mark. The same frame holds the playlist naming row,
  the zone's name, the matching sheet's two fields and the Services pane's search; the header's
  search keeps its own `raised` fill, at the frame's height and rounding.
- **A page inside a category opens from a nav row and is left by the way back or escape.** A heavy
  tool is a `Page`: the Equaliser's *Headphone correction* (AutoEq and what is plugged in, the
  preferred measurers), *Your own measurement*, *Measure with a microphone*, *Room correction* (the
  response, where the speakers stand) and *Listening test*; Library's *Rescan, refresh and retag*
  (the files, what online knows, the tags in the files); Sync's *Your devices* (the devices and
  driving them, what they may drive here). Each is a `kit::nav_row` (`settings-page-<id>`: a bare mark in
  `muted`, the title, `Page::about`, a chevron) in a card titled by `Page::gathered_under`
  where the page's first group stands in the table. `RootView::settings_page` holds the page beside
  the category (`open_the_settings_page`, `leave_the_settings_page`; `show_settings` lets it go, so a
  rail press or another category returns to the category's own page); the heading reads the page's
  title and `about` under `kit::way_back` (`settings-way-back`, *‹ Equaliser*) in place of the
  *Settings* eyebrow, and escape with nothing else to dismiss and the filter empty goes back
  (`dismiss_search`), before any step back through the window's pages
  (`a_correction_opens_on_a_page_of_its_own_and_the_way_back_and_escape_return_to_the_equaliser`).
  The page is not remembered between runs; the category is.
- **Hidden songs are brought back from Filters** (`Group::Hidden`, `filters.rs`'s `hidden_group`):
  the songs hidden from their menus, listed by title off `LibraryModel::hidden` (read with the
  standing part of every whole read, `Library::hidden_tracks`, at most `HIDDEN_LISTED_AT_MOST`
  (200), the count said beside them where more are hidden), each with *Show again*
  (`hide_track(id, false)`); *Show them all again* is one edit (`show_every_hidden_track`) and *List
  them in Tracks* puts `is:hidden` in the search. Nothing is a setting, so the group puts nothing
  back (`a_hidden_song_is_listed_in_the_filters_and_shown_again_by_one_press`).
- **Filters** (groups `MusicExtensions`, `MinimumLength`): a wrapping grid of `kit::checkbox`es,
  all ticked by default; *Select
  all*/*Clear selection* greyed where they would change nothing. Minimum: seconds field, 0 to 600,
  applied on Enter or leaving (a positive minimum excludes tracks of unknown length); a refused number
  stays in the field and toasts; Escape restores the held value.
- **Metered connections and the renderers' FLAC are groups of their own.** `Group::Metered` (Online)
  is two `switch_row`s, `enrich-on-metered` and `stall-downloads-on-metered`, held in
  `ResonateApp::online` (`Online::{enrich_on_metered, stall_downloads_on_metered}`) and applied live
  by the binary from the `SettingChange` (`binary.md`); it draws a line while
  `Library::is_waiting_on_the_meter` (the poll's `PollProgress`) holds, and its mark puts both off.
  `Group::FlacRenderers` (Output) is a `switch_row` per sink whose `NodeName::names_a_upnp_renderer`
  holds, then one per name in `ResonateApp::flac_above_96k_for` that no sink answers (so a renderer
  that has left can still be unnamed); it stores the whole set as `Setting::FlacAbove96kFor`, says it
  is read from the next start, and its mark empties the set.
- **The library's folders are one box, and the primary one is chosen in it.** `Group::Folders`
  (*Music folders*, Library category) lists every root as a row of its card: a radio, the folder, a
  `PRIMARY` badge on the chosen one and the forget mark. Pressing a card's radio or name makes that
  root `music-folder` (checked off the window's thread, then `set_the_music_folder`); pressing the
  primary again chooses none. A `music-folder` that is no root itself (inside one, or waiting to be
  added) is drawn as a card of its own with a mark that clears it. Forgetting a root holding the
  primary clears the primary. *Add folder…* and the typed field add roots; the *File dropped songs
  by the layout* switch ends the box, and the group's mark puts back no primary and the switch on.
  **With no primary folder the Tracks pane offers to set one**: its empty state says *Add songs to
  start your library* and a *Set primary folder…* button opens the folder dialogue
  (`choose_the_primary_folder`).
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
  `sync.md`): *Device sync*, *Pairing*, the *Your devices* page (*Devices*, and *Driven by your
  other devices*), *Connection* and *Songs between devices*:
  - *Device sync*: the switch and a standing row (a tinted mark, `done`, accent, `failure` or
    muted by `Mood`, a headline and a line under it read off `SyncShown` in the one label style; *Sync now* while syncing),
    and the songs on their way, said as both numbers: *342 songs on their way from another device
    · 2 downloading · 340 waiting* (`on_their_way`); the phones head their transfers the same way
    (`transfersHeader`, `fetchingHeading`).
  - *Pairing* (keyless): a `pairing_tile()` square that stays a cover (*Click to show*) until the
    listener shows it, then the QR code of `SyncShown::link` (`resonate://pair/<code>`, the `qrcode`
    crate, level M, painted as quads in runs of dark modules on `scanned_paper` with `scanned_ink`,
    the one place a colour is not the palette's: a scanner wants black on white); beside it the code,
    covered by `•` until shown, with *Show*/*Hide*, *Copy* and *New code* (armed by the first press,
    `Armed::RegenerateTheCode`; the second clears `sync-key`, and the binary's sync watcher mints a new code at once), then *Join another
    device's code* (read by `Stored::reads_as_a_code`, the binary's `SyncKey::from_code`, whether or
    not sync runs). `RootView::pairing_shown` falls back to covered with every armed press lowered
    (leaving the category, a page or the pane, or the pointer leaving the body) and after a new code.
  - *Devices* (keyless, on the *Your devices* page): this device first, then online before away, each a row of the card with
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
- **A boolean is a `kit::switch_row`, a choice a `kit::segmented`; only a device or palette is still a row of its own.**
  A switch names what it does and what off means, whole row = hit area; a `Choice` is a trough of
  segments, each a radio in the ring (`option_in_the_ring`, `option_named` for a segment named at
  run time: a binding's and a band kind's), so every one takes `space`/`enter` and wears the ring.
  Vocabulary: `kit::group`, `rows`, `row`, `stacked_row`, `field`, `nav_row`, `switch_row`,
  `framed_field`, `action_row`, `segmented`, `switch`, `preview`, `dot_swatch`, `choice_row` with its
  `radio`; the pane draws none itself. **`kit::segmented` answers with the trough
  itself; the caller hugs it** (a wrapper round the trough put every segment beside it): watch for
  builders returning a parent their children will not reach.
- **A device reads as what it is and what it takes; its node name is behind the pointer.** Each choice,
  *Follow the system default* included, is a `kit::choice_row`: `kit::radio` held
  `kit::level_with_the_choice` beside a column whose first line is a `kit::choice_name` (`flex_1`, ends
  in an ellipsis; badges `DEFAULT`, `UNPLUGGED`, `IN USE`, the playing track's verdict `TAKES … AS IS`/
  `CONVERTS` follow on the same line). Each is a row of the group's card, bled to its cell; the chosen
  row has an accent wash (`kit::chosen_wash()`) and the filled radio as its only marks. Under the name, two `kit::details` lines divided by faint `·`: how the
  sink is reached, in words (port, the card's profile, whose volume it is), and what it takes, in
  `kit::figure`s (depths/rates via `format::depth` and `SampleRate::kilohertz`, every digit the rate
  holds, so 22.05 kHz is not 22.0). The node name is what the settings file holds, so the row `names`
  itself by it; `SinkInfo::is_hardware` is drawn nowhere.
- **A group holds a subject, not a control** (theme shelf and accent swatches = one *Colour* group).
  A group's hint is written once, in the table; the heading's info mark is its only drawing (a gpui
  tooltip is built from an `AnyView`, not a string).
- **Every group ends in a line saying what it does; a choice says what the chosen option does.**
  `Choice::meaning` is required; `RootView::choices` draws the chosen option's meaning under its trough
  as a `note`; `detail` stays the hover for figures such as the filter's taps. A list/button-row group
  (devices, music folders, scan, bindings, About's readbacks) ends in a `note` of its own, always the
  one `note` builder at `text_sm`. *Lossy sources* = *Artificially enhance lossy files*, with the
  *Experimental* badge `Group::is_experimental` puts in a header; its options say in words what they
  take or add, and its note (`lossy-only`) says that a repaired file is no longer bit-perfect.
- **A palette is shown, not named.** `kit::preview` = a strip of four bands (sidebar, panes, what it
  raises above them, the accent that would be worn) under the palette's name in a card with an accent
  border when worn. An accent is a round `kit::dot_swatch` with a check in `theme::ink_over`'s ink.
  Appearance and Window are the categories with nothing behind them but the window: a choice is worn
  via `theme::wear` or a `ResonateApp` field, saved through the `Settings` seam, redrawn by
  `cx.refresh_windows()`; no `Command`. Appearance's groups in order: *Colour* (a switch *Follow the
  desktop's light or dark setting*, the palette cards, the accents); *Layout* (a switch *Follow the
  desktop's text size*, then the text size: Small, Medium, Large and Larger, roots of 12, 13.5, 15.5
  and 19 against the 13.5 middling one, so Larger reads at 141 per cent, and Largest at 27, 200 per
  cent); *Motion* (`reduced-motion`, `ResonateApp::reduced_motion`, `RootView::reduce_motion`;
  `motion.md`); *Scrollbars*: Always / While scrolling (`ScrollbarMode::AutoHidden`, default) /
  Never (`ResonateApp::scrollbars`); *Live spectrum*. Window's: *Window buttons* (worn via
  `RootView::show_window_buttons` onto the global, not `theme`); *Sidebar tabs*: `suggestions-tab`,
  `missing-tab`, `tab-counts` onto `ResonateApp::tabs` via `RootView::show_tabs`; *Window state*
  (`remember-*`); *The volume wheel* (`ResonateApp::scroll_volume`, `scroll-volume`); *Mouse
  navigation* (`mouse-navigation`). Playback holds *Resuming*, *Repeating a track* and *The
  previous button*; Accounts every sign-in and server key, Last.fm and ListenBrainz (*where your
  plays are scrobbled*) among them.
- **A remembered category reads every spelling a build has written.** `SettingsCategory::parse`
  takes each category's own word and `SPELT_BY_EARLIER_BUILDS`: `enrichment` opens Library (whose
  *Rescan, refresh and retag* page holds what it held), `scrobbling` Accounts (which holds
  ListenBrainz now), so an older `last-settings-category` lands where its settings went (a test per
  spelling, `settings.rs`; the binary's reader takes them too, `binary.md`).
  `Pane::is_shown` is what a tab being off means: sidebar and `stepped_pane` skip the pane, `set_pane`
  lands on Tracks instead, hiding the front pane moves off it at once.
- **gpui scrolls a region and draws no bar, so `views/scrollbar.rs` does.** `Scrollbars::of` reads the
  setting where a pane is built; `vertical`/`horizontal`/`around` answer a bar over the region's edge,
  or an empty absolute div under `Hidden`. A `horizontal` bar is inset `SHELF_INSET` (24) each side. A list that scrolls inside a
  sheet or a panel carries one as a pane does: the downloads list, the Sort pop-up's orders, the
  matching sheet's recordings, the Listen sheet's *Heard earlier*, the playlist picker's rows. A
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
  groups from *every* category and page, whole and working, under a link naming where they are kept
  (`kit::place_link`, `settings-place-<category>-<page or main>`: *Equaliser · Headphone
  correction*); pressing it clears the query and opens that page
  (`a_setting_found_on_a_page_of_its_own_opens_that_page_and_leaves_the_search`). **Ranked by how
  they answered**: a group answers each word by title, other name, its category's or page's name, or
  hint, in that order of strength, weighed as its weakest word; a category leads by its best group and
  a page within it by its own, so no category or page is drawn under two links
  (`a_setting_named_by_the_words_leads_one_whose_hint_merely_mentions_them`,
  `a_setting_on_a_page_of_its_own_is_found_by_its_name_and_by_the_pages`).
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
  **The row whose menu is open stays lit.** The menu's scrim occludes the pane, so the row loses its
  hover and its star would vanish while the listener chooses an entry. `Menu::opened_on` records the pressed element's id
  (`opens_a_menu`), `RootView::render` publishes it as the `menu::Opened` global, and a row asks
  `menu::lit_by_its_menu(id, cx)` to draw its wash (every menu-opening row, by `opens_a_menu`) and a
  track row to hold its star at full opacity
  (`the_row_whose_menu_is_open_stays_lit_so_its_controls_do_not_vanish_under_the_scrim`).
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
  (`QueueLength::unmeasured`); a sort keys every row in the background too (`ordered_rows`),
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
    takes no caret on a click. A row's controls are `RootView::row_controls` (a handle that is no
    tab stop, `Controls::holding`), drawn always
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
  **One press is armed at a time**: `RootView::armed` is an `Option<Armed>` (the filing's move and
  walk back, the tags' write and walk back, the reset, the vault's keep, the curve discard, history
  aging, a folder's forget, a new pairing code, the earlier plays), `arm`, `is_armed` and `lower`
  its only gestures, so arming one lowers any other; `disarm` (leaving the pane, a category or a
  page) and the pointer leaving the settings body (a transition via `on_hover`) run
  `lower_every_armed_press`. Passes run through `Work`, read by `LibraryModel::is_busy`: scan, forget, organise,
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
- **Everything that brings the catalog up to date is one Library page, in three groups.** *Rescan,
  refresh and retag* (`Page::Enrichment`): *The files* (`Group::Rescanning`: *Rescan folders*, *Read
  every file again*, *Stop* while a scan runs, the scan's progress and counts), *What online knows*
  (`Group::LookingUp`: *Look up*, *Refresh all*, *Speed up once*, *Look for missing covers*, *Stop*
  while a lookup runs, the breakdown) and *The tags in the files* (`Group::Retagging`: the armed
  *Preview*/*Apply* pass below), each a group with a hint of its own. *Read every file again* (`Reading::Everything` beside `Prompted` on
  `LibraryModel::start_scan`) re-probes every file whatever size and mtime say, each row keeping id,
  plays, favourite; *Look for missing covers* is `Library::ask_again_for_covers` + a lookup with studies and lyrics off (`LibraryModel::only_the_covers`), so the button never starts the study pool's decoding; *Refresh
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
  Order: the signal path as one card of three columns (*Source*: codec in the lossless colour over
  depth, rate, channels, container; *Processing* in words, a short headline (*Untouched*, *Repacked*,
  *Dithered*, *Resampled* where the rate moved, else *Converted*) over what it went to (*To 16-bit /
  48 kHz*, *To 48 kHz by the graph*); *Output*: mode in its colour over what was negotiated and the
  sink; each a muted label over a `text_lg` semibold headline), then the detail cards in one column
  (above), the bitrate graph after the Bitrate card, tags last.
  `inspector::fields` is a property list: a muted label and its value at the same `text_sm`, a
  `soft_edge` rule between rows, labels in sentence case. Follows the *playing* file only. The live profile restarts
  on every rebind (`StreamDigest::profiled_from`: a `ProfileBuilder`'s windows are unresumable
  mid-span); the engine publishes a new digest only after the profile has grown by a sixteenth
  (a step of one to thirty windows, `digest_is_due`), and the decoder runs ahead of the sink, so
  the graph leads what is heard. **A digest that only grew** (`StreamDigest::only_grew_from`: same
  track, file, info, gain and profile start) is `Moved::Clock` in `PlayerModel::refresh`, so the
  header, sidebar and pane are not redrawn for it, only the bar and an inspector or analysis pane. A line, not bars: `plot::stroke` over a `plot::wash` from the accent to
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
  reading back codec, declared depth and rate, or how far the decode has got. Then: the verdict as a
  word in its colour at `text_lg` (*Genuine*, *Not judged*; never a tinted badge)
  over one sentence per finding (`Finding::told`); the findings under a `soft_edge` rule, each its description led by a mark in its severity's colour (`Icon::Alert` in the failure colour for a fault, in `suspect` for a caution; `Icon::Info` in muted for a note, its words muted too), centred on the first line so a wrapped description keeps its alignment, the severity word kept for a screen reader; waveform (lane per channel, min-max polygon washed in
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
  note names the source in the stage, not the one chosen; source segments and microphone rows take no
  press while `is_listening` and `listen_from` refuses one
  (`a_source_pressed_while_recording_is_refused_and_the_recording_names_its_own`).
  **The sheet reads top down from what is happening to what it is set to.** Under the heading (the
  ear in the accent, *Listen*, the close mark) stands the stage, then its gestures in
  `kit::sheet_actions`, then under a
  `soft_edge` rule the settings (*Listen to* with *Desktop*/*Microphone*, the microphones, *Keep
  listening* saying under it what following is doing), then under another rule *Heard earlier*.
  **Every stage is one shape** (`staged`): a `theme::listen_cover()` square on `raised`
  (`stage_tile`) beside its words, centred on it, so the sheet never changes height between stages
  and a named song's cover lands where the listening mark stood. **Every ending is one `Ending`**
  (`ending_of`, pure, tested): the tile's mark in its weight's colour (the ear in the accent for
  *Ready to listen*, `Icon::Info` muted for an aside, `Icon::Alert` in `failure` for trouble), a
  headline (no full stop), a sentence (ending in one) and
  what comes next: *Listen* or *Listen again* (`Tone::Primary`), *Open the settings* for *Online is
  off* (closes the sheet onto the Online category,
  `online_off_offers_the_online_settings_and_leaves_the_sheet_for_them`), nothing for *Nothing can
  name songs yet*. A failed recording names the source it was taken from. Recording draws
  *Listening to the desktop…* (or the microphone's own description), the seconds left as a
  `kit::figure` (`seconds_left`, rounded up), a rail and the try under it, the ear on its tile;
  asking spins on its tile over a full rail; both offer *Stop*. A found song's tile is its cover (a
  disc mark where the service sent none), and beside it *Named by Shazam*
  (`service_named` spells the service), title at `text_xl`, artist, album and year; then *Open* where
  the service gave a link, *Listen again* (`Tone::Outlined`), *Find it* (`search_instead` +
  `choose_pane(Pane::Tracks)`). The stage is a live region keyed by the stage (a status; an alert
  for trouble), so each stage is heard once. **A microphone nobody connected is said to be missing**:
  once the list is read and empty (`ListenModel::hears_no_microphone`) the microphone choice says
  *No microphone is connected.* in the failure mark's colour and *Listen* is greyed
  (`a_microphone_nobody_connected_is_said_to_be_missing_and_the_default_one_can_be_chosen_again`).
  The last `HEARD_KEPT` (8) songs stay listed for the run, *Heard earlier* listing every one but the
  song the stage shows (title, artist muted, a search mark lit under the pointer).
  *Desktop*/*Microphone* = a `kit::segmented`; with more than one microphone `kit::radio_row`s of
  *Default microphone* (`Listening::Microphone(None)`) then each PipeWire offers (listed each time the
  sheet opens), with one its description alone, written back as `listen-from`; Online's *Listening*
  group writes the same key and `listen-for`; *Recognition* has the AudD token beside the AcoustID key
  (read at the next start); *ListenBrainz* is the `listenbrainz-token` field, not read at next start (the
  binary's submitter follows the settings file). With a token typed, *Send earlier plays* is armed by
  the first press (`Armed::TellTheEarlierPlays`) and asks `Library::tell_earlier_listens` on the second
  (`the_earlier_plays_are_asked_for_by_the_second_press_alone`). **A token given is asked about at
  once**: where Online is on and the build handed `Lookups::scrobblers`, `check_the_listenbrainz_token`
  asks `token_held` on bg and toasts whose token it is or that the service does not know it.
- **Play next and Add to queue are row actions; Shuffle is the tracks heading's second play action.**
  Each track row, the playlists index row and each opened-playlist row carry the queue pair (on a
  playlist row they take the whole reach the row is in). The + on a queue row and an opened-playlist row
  opens the playlist picker; index and opened-playlist headings use + to enter track-browsing mode.
  Albums and artists panes, the queue pane, the album page, artist page and opened suggestion carry
  neither. Named once in `words::menu::play_next`/`add_to_queue`: *Play next* = `Placement::Next`; *Add to
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
- **Every major listing is ordered from a dialogue and a header.** Tracks, albums, artists each hold an
  order and a reading on `LibraryModel::sorting`, kept for the run. Tracks start in date-added order,
  newest first, so newly scanned or synced songs appear at the top. Two writers: a sort icon opening the
  Sort dialogue, and the pressable column header (a press names the order; a second on the one
  in force reverses it; the column in force wears the accent and a chevron). `views/sorting.rs`:
  `Ordering` is the trait the five order enums implement. **The dialogue is one pop-up**
  (`RootView::ordering`, an `Ordered` naming what it sorts: tracks, albums, artists, playlists, a
  playlist's rows, the queue, a search being saved; `Sheet::Ordering`): `order_sheet` over
  `order_choices`, the orders as `kit::checked_row`s (scrolling where a short window cuts them) and the
  direction as two `kit::segment`s worded for the order held (*A to Z*, *Newest first*), a playlist's rows
  adding *Keep this order* under a rule. A press or key applies at once and leaves it open to take the
  direction too; enter, a press outside or escape close it, the focus going back to what opened it, the
  name field while a search is named
  (`the_sort_mark_opens_a_pop_up_under_itself_that_orders_the_tracks_and_leaves_by_every_way_out`).
  `listing::columns` takes a `Sorted` descriptor (columns offered, which is in force, a `fn` pointer for
  the press), so tracks pane, queue and opened playlist draw one header. A right press on a header
  offers every order.
- **The queue is ordered as a gesture, not a standing order**: each press in its Sort dialogue sends one
  `Command::Order` (a permutation over the rows the window draws); the engine rewrites the play order in
  one pass with the cursor re-seated, so a mid-track sort reopens no stream. *Clear* sends one
  `Command::Remove` over the whole span. The reach lives for the run, escape clears it.
- **Undo and Redo are drawn in the playlists headings only** (`RootView::undo_edit`/`redo_edit`; Redo only
  while a step is there; each says how many steps stand behind the one it offers, `Undoable::behind`).
  Rows put in a playlist from the tracks or queue pane have no control to press, so the notice names
  `ctrl-z`. **A narrowed playlist says what a gesture reaches** (`words::playlists::narrowed_hint`): Play, Play next, Add
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
  answers nothing and the tick is passed over. **A model's playlist edits come with it onto *Undo***:
  every such reload, and the watch's start, first runs `LibraryModel::take_what_a_model_offered`
  (`Library::take_what_a_model_offered` on the background executor, chained like the edits,
  notifying where it took any; `library.md`). Undo, Redo and their toasts say a model made the
  step (`made_by` in `views/playlists.rs`, `made_by_saying`)
  (`an_edit_a_model_made_beside_the_window_is_taken_onto_its_undo_and_put_back_there`).
- **`Raise` activates the window; `Quit` closes it.** The binary's host answers `can_raise` with whether
  it holds a raise channel (the window's does; windowless `resonate play` and the bus's default `Host`
  answer false); `Raise` sends on it, `raise_when_asked` drains it on the gpui foreground and calls
  `activate_window` on every window (a compositor may still refuse without an activation token). `Quit`
  sends on the channel `resonate play` uses, drained by the gpui foreground on the frame timer.

The desktop collection orders Favourites, Playlists, Suggestions and Listening Party before Missing. The enrichment status opens the Library category's *Rescan, refresh and retag* page, with live lookup counts and cancellation alongside scan and tag controls. Accounts holds service sign-ins, server credentials and scrobbling, including Spotify, TIDAL, Subsonic, Jellyfin, Last.fm and ListenBrainz; Online retains network policy.

Other devices playing appear as compact cards at the sidebar foot across panes (`views/remote.rs`):
platform mark, device name and playing or paused state on one line, cover beside title, artist and
album, then position beside previous, play or pause and next. The clock and controls share the
control's height, with the clock's text line centred inside it. Padding and gaps stay tight; the zone is
named by the card and each control's hint. A one-second window task watches online paired devices
through the existing remote-control seam; only an accepted mirror offers a card and controls, so a
device refusing remote control offers none. Controls address that device and zone through
`press_on_the_device` on the background executor and keep their own pruned keyboard ring across pane
changes. Album names and covers come from a matching recording in the desktop catalog, resolved off
the window thread and reused until the recording or catalog names stamp changes; an unheld recording
draws a disc and omits the album. The card list scrolls within a share of the window's height, keeping
Settings reachable. The *Other devices* heading is a keyboard-accessible disclosure: it hides or
shows the cards for the run and reports its expanded state to a screen reader. While the cards fit
they stand in the sidebar's foot. After layout, if the categories above need to scroll while the
cards stand there (the actual overflow, pinned rows and live status included, not a window-height
threshold), the panel turns floating until the window is that overflow taller again
(`remote_fits_from`, `RootView::remote_floats`): the cards collapse, unless the heading was pressed
open that very frame, and once opened they draw as a raised, shadowed card over the categories
above the heading (`remote-devices-floating`), taking no room, so nothing can oscillate. A press
anywhere but the card closes it; a press on the heading is told apart by its position
(`remote_heading_pressed_at`) so it toggles once rather than closing and reopening
(`devices_without_room_float_over_the_categories_until_pressed_away_and_return_once_they_fit`).
Watches are released when the device, sync service or window goes away.

A song is rated explicit by its file content advisory, provider rating or title qualifier; a clean advisory overrides a title qualifier and an unmarked title is never assumed clean. No row draws the rating. A held track menu offers clean and explicit version searches using the title without its rating qualifier and the artist. These searches change no files; results still need the listener to choose a recording.

A song row nests Share into song.link and Upload file to Catbox (public). The upload runs on the background executor and copies the returned public URL, with progress and failure toasts. Only whole local files are uploaded; cue spans and remote sources are refused rather than exposing an entire backing file.
