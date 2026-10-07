---
paths:
  - "crates/resonate-ui/**/*.rs"
---

# Motion

Short and quiet: confirms a change without making the listener wait. `motion.rs` is the vocabulary;
views name no duration or curve outside it (exceptions with their own: the lyrics' glide, the
toast, the scrollbar thumb).

## The vocabulary

- **Spans**: `ARRIVES_OVER` 150 ms (whatever appears), `FLIPS_OVER` 140 ms (control state change),
  `HANDS_OVER` 280 ms (song change), `HINT_ARRIVES_OVER` 100 ms (hover hint, asked for by resting
  the pointer: already a wait).
- **Curves**: `settling` (cubic ease-out, most movement in the first third = snappy); `smooth`
  (smoothstep; song handover, neither end may jump); `overshooting` (favourite star, the one motion
  allowed past its end).
- **`lifted_in`**: fade in + rise `LIFT` (4 px) via `relative().top` (moves paint, not layout, so an
  `anchored` menu measures and snaps to the window as at rest). **`risen_in`**: same for an
  absolute element placed by `bottom` (`relative()` would unplace it). **`faded_in`**: opacity
  only (scrim, pane body, hint).

## What arrives

Lift in: menus, release and genre card, magnified cover, listen and delete sheets, playlist
picker, drop overlay, type-ahead and copying pills, downloads panel. A sheet's scrim fades in
separately, its card lifts inside. Keys are fixed names: gpui keeps an animation's start under the
element id and drops state no frame reads, so a surface closed for a frame and reopened replays;
one redrawn while open (menu stepped by arrows, card whose pressings arrive) does not. A hint is a
new entity per show, so it fades in each time.

- **Pane body fades in**, keyed by the pane (tracks pane: the scope, an album or artist page being
  a new page; plus whether search results stand in front). The clock's per-tick redraw keeps the
  key, replays nothing. Settings column: keyed by category, or *found* while narrowed.
- **Nothing waits on the way out**: a dismissed surface leaves that frame (a closing animation
  would hold a backdrop after the listener moved on). Toast excepted: nobody dismissed it.
- **Hover is not animated**: gpui resolves `hover`/`group_hover` in paint; a transition would mean
  tracking the pointer per row of every list.

## What flips

gpui's `with_animation` plays on first render too, so a state-keyed switch animated on opening
Settings. **`Flip`** keeps the last drawn value in element state and moves only on change, from
`Flipping::towards`; a change back before landing runs home from where it stood
(`a_flip_turned_back_midway_runs_home_from_where_it_stood`). `turned` reads a boolean pair as a
share; `blended` lerps two colours.

- `kit::switch`: knob thrown between two growing spacers, track and knob blended; animation on a
  child so the track stays the `Stateful<Div>` `in_the_ring` takes.
- `kit::segment`, settings rail, shuffle/repeat/queue toggles: ground layer fades in under the
  label (`shown_while`); a child, not the control's background, so callers keep building on the
  control. Sidebar chosen row likewise, accent bar growing from its middle.
- `kit::radio` grows its dot. Play/pause, repeat glyph, mute mark `popped` on change.
- `kit::star`: newly favoured star scales past its size and back, keyed by the `Favoured` it marks,
  not the row id (a row redrawn over another track after a sort must not pop).

## The song change

A skip hands the panel over, not redraws it. `Handover` keeps shown and leaving, with a serial
bumped per change for animation keys; drops the leaving once `HANDS_OVER` is out
(`a_skip_keeps_the_last_song_beneath_until_the_handover_is_out`).

- **One pace**: words and cover share `HANDS_OVER` and curve; `arriving_opacity` is `smooth`,
  `leaving_opacity` its mirror, so leaving is exactly as fast as arriving
  (`the_leaving_song_goes_at_the_pace_the_arriving_one_comes`). Text once ran a staggered pair (gone
  by 40 %, back from 30 %); beside a cover easing the whole span it read as two changes.
- **Words**: keyed by title, artist, album. Leaving song drawn beneath, same place, own id, fades
  out; arriving on top, keeps the presses, fades in. Nothing slides.
- **Cover**: keyed by what it pictures. Leaving face stays opaque beneath, arriving (art or disc)
  fades in over it: a true crossfade because a cover is opaque.
- **Signal path held between songs**: the engine publishes no current track while the next row
  opens; the path once dropped then, and the centred text column's title and by-line fell half a
  line and rose again with the next song, reading as text arriving from below.

## Scrollbars

`AutoHidden` thumb fades in over `THUMB_ARRIVES_OVER`, out over the last `THUMB_LEAVES_OVER` of
`SCROLLED_LINGERS`, read in paint from when the scrolling run began (`Scrolled::since`) and when it
last moved. Paint asks for frames only while the thumb is part way; the scroll's one timer wakes the
window as the fade-out begins.

## Frames

An animation asks for the next frame of its own view; the window is four cached `Part`s, so a
flipping switch redraws the pane, not the transport. Every motion is at most a few hundred ms;
nothing asks for frames once landed.
