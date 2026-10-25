---
paths:
  - "crates/resonate-ui/**/*.rs"
---

# Motion

Short and quiet: confirms a change without making the listener wait. `motion.rs` is the vocabulary;
views name no duration or curve outside it (own exceptions: the lyrics' glide, the toast, the
scrollbar thumb).

## The vocabulary

- **Spans**: `ARRIVES_OVER` 150 ms (whatever appears), `FLIPS_OVER` 140 ms (control state change),
  `HANDS_OVER` 280 ms (song change), `HINT_ARRIVES_OVER` 100 ms (hover hint: summoning it is
  already a wait).
- **Curves**: `settling` (cubic ease-out, snappy); `smooth` (smoothstep, for the song handover:
  neither end may jump); `overshooting` (the favourite star, the one motion allowed past its end).
- **`lifted_in`**: fade in + rise `LIFT` (4 px) via `relative().top`, so an `anchored` menu still
  measures and snaps to the window as at rest. **`risen_in`**: same for an absolute element placed
  by `bottom`. **`faded_in`**: opacity only (scrim, pane body, hint).

## What arrives

Lift in: menus, release and genre card, magnified cover, listen and delete sheets, playlist picker,
drop overlay, type-ahead and copying pills, downloads panel. A sheet's scrim fades separately, its
card lifts inside. Keys are fixed names: gpui keeps an animation's start under the element id and
drops state no frame reads, so a surface closed for a frame and reopened replays, one redrawn while
open does not. A hint is a new entity per show, so it fades in each time.

- **Pane body fades in**, keyed by the pane (tracks pane: the scope, plus whether search results
  stand in front); the clock's per-tick redraw keeps the key. Settings column: keyed by category,
  or *found* while narrowed.
- **Nothing waits on the way out**: a dismissed surface leaves that frame, so a closing animation
  never holds a backdrop after the listener moved on. The toast is excepted: nobody dismissed it.
- **Hover is not animated**: gpui resolves `hover`/`group_hover` in paint; a transition would mean
  tracking the pointer per row of every list.

## What flips

gpui's `with_animation` plays on first render too, so a state-keyed switch animated on opening
Settings. **`Flip`** keeps the last drawn value in element state and moves only on change, from
`Flipping::towards`; a change back before landing runs home from where it stood
(`a_flip_turned_back_midway_runs_home_from_where_it_stood`). `turned` reads a boolean pair as a
share; `blended` lerps two colours.

- `kit::switch`: knob thrown between two growing spacers, animation on a child so the track stays
  the `Stateful<Div>` `in_the_ring` takes.
- `kit::segment`, settings rail, shuffle/repeat/queue toggles: ground layer fades in under the
  label (`shown_while`), a child so callers keep building on the control. Sidebar chosen row
  likewise, accent bar growing from its middle.
- `kit::radio` grows its dot. Play/pause, repeat glyph, mute mark `popped` on change.
- `kit::star` (`motion::starred`): a newly favoured star scales past its size and back, keyed by the
  `which` it marks, not the row id (a row redrawn over another track after a sort must not pop).

## The song change

A skip hands the panel over, not redraws it. `Handover` keeps shown and leaving, with a serial
bumped per change for animation keys; drops the leaving once `HANDS_OVER` is out
(`a_skip_keeps_the_last_song_beneath_until_the_handover_is_out`).

- **One pace**: words and cover share `HANDS_OVER`; `arriving_opacity` is `smooth`,
  `leaving_opacity` its mirror (`the_leaving_song_goes_at_the_pace_the_arriving_one_comes`), since a
  staggered text pair beside an eased cover read as two changes.
- **Words**: keyed by title, artist, album. Leaving song beneath, same place, own id, fades out;
  arriving on top, keeps the presses, fades in. Nothing slides.
- **Cover**: keyed by what it pictures. Leaving face stays opaque beneath, arriving fades in over
  it: a true crossfade because a cover is opaque.
- **Signal path held between songs**: the engine publishes no current track while the next row
  opens; dropping the path then made the centred title and by-line fall half a line and rise again.

## Scrollbars

`AutoHidden` thumb fades in over `THUMB_ARRIVES_OVER`, out over the last `THUMB_LEAVES_OVER` of
`SCROLLED_LINGERS`, read in paint from when the scrolling run began (`Scrolled::since`) and when it
last moved. Paint asks for frames only while the thumb is part way; the scroll's one timer wakes the
window as the fade-out begins.

## Reduced motion

`reduced-motion` (Appearance, *Motion*; default off) is `ResonateApp::reduced_motion` and gpui's own
`App::reduce_motion`, set at start and by `RootView::reduce_motion`. gpui renders every
`with_animation` (the lift, rise and fade helpers, the hint, the toast, the song handover) at its end
state and asks for no frames, so a surface arrives at once. What does not go through
`with_animation` asks itself:

- `Flip` reads `cx.reduce_motion()` and `Flipping::moves_to` lands at once
  (`a_flip_under_reduced_motion_lands_at_once_and_asks_for_no_frames`): switches, segments, popped
  glyphs, the star.
- The scrollbar thumb is whole while the scroll lingers and gone when it would have begun to fade
  (`Scrolled::presence`).
- The lyrics pane keeps still (`LyricsModel::keep_still`, set each frame): `glide_to` sets the offset,
  lines do not rise on arrival (`rise`, `is_arriving`). The colour turns of a lit line and the
  sung-word spread stay, being timing information and not movement.
- The live spectrum and the visualiser are data, not decoration, and keep moving.

## Frames

An animation asks for the next frame of its own view; the window is four cached `Part`s, so a
flipping switch redraws the pane, not the transport. Nothing asks for frames once landed.
