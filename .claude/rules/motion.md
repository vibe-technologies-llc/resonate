---
paths:
  - "crates/resonate-ui/**/*.rs"
---

# Motion

Every movement in the window is short and quiet: it confirms a change without making the listener
wait for it. `motion.rs` is the vocabulary, and a view never names a duration or a curve of its own
outside it, the lyrics' glide and the toast being the two older motions that keep theirs.

## The vocabulary

- **Three spans.** `ARRIVES_OVER` (150 ms) for whatever appears, `FLIPS_OVER` (140 ms) for a control
  whose state changes, `HANDS_OVER` (280 ms) for the song change. `HINT_ARRIVES_OVER` (100 ms) is a
  hover hint's, a hint being asked for by resting the pointer and so already a wait.
- **Curves.** `settling` (a cubic ease-out) carries most of the movement in the first third, which
  is what reads as snappy; `smooth` (smoothstep) is the cover crossfade's, where neither end may
  jump; `overshooting` is the favourite star's, the one motion allowed past its end.
- **`lifted_in`** fades an in-flow element from nothing and rises it `LIFT` (4 px) through
  `relative().top`, which moves what is painted without moving the layout, so an `anchored` menu
  measures and snaps to the window as it would standing still. **`risen_in`** does the same for an
  absolute element placed by `bottom`, a `relative()` there would unplace it. **`faded_in`** is
  opacity alone, for a scrim, a pane body and a hint.

## What arrives

Menus, the release and genre card, the magnified cover, the listen and delete sheets, the playlist
picker, the drop overlay, the type-ahead and copying pills and the downloads panel lift in; the
scrim behind a sheet fades in on its own and the card lifts inside it. Each is keyed by a fixed
name: gpui keeps an animation's start under its element id and drops any state no frame reads, so a
surface closed for a frame and opened again replays, and one redrawn while open — a menu stepped by
the arrows, a card whose pressings arrive — does not. A hint is a new entity each time it shows, so
it fades in each time.

**A pane body fades in** under a key of the pane, the scope for the tracks pane (an album or an
artist page is a new page) and whether search results stand in front. The clock redrawing the pane
every tick keeps the key and replays nothing. The settings column is keyed by its category, or by
*found* while narrowed.

**Nothing waits on the way out.** A dismissed surface leaves the frame it is dismissed: a closing
animation would hold a backdrop over the window after the listener has moved on. The toast is the
exception because nobody dismissed it.

**Hover is not animated.** gpui resolves `hover` and `group_hover` in paint, so a transition would
mean tracking the pointer for every row of every list.

## What flips

gpui's `with_animation` plays on an element's first render as well as on a change of key, so a
switch keyed by its state played every switch the moment Settings opened. **`Flip`** is the element
for a state change: it keeps the value it last drew in element state and moves only when the value
changes, from `Flipping::towards`; a change back before the last one landed runs home from where it
stood rather than jumping (`a_flip_turned_back_midway_runs_home_from_where_it_stood`). `turned`
reads a pair of booleans as a share and `blended` lerps two colours.

- `kit::switch` throws its knob between two growing spacers and blends track and knob, the animation
  on a child so the track stays the `Stateful<Div>` `in_the_ring` takes.
- `kit::segment`, the settings rail and the shuffle, repeat and queue toggles fade a ground layer in
  under the label (`shown_while`), the ground being a child rather than the control's own background
  so callers keep building on the control. The sidebar's chosen row does the same, its accent bar
  growing from its middle.
- `kit::radio` grows its dot. Play and pause, the repeat glyph and the mute mark `popped` on a change.
- `kit::star` scales a newly favoured star up past its size and back, keyed by the `Favoured` it
  marks rather than by the row's id, so a row redrawn over a different track after a sort does not
  pop.

## The song change

A skip hands the panel over rather than redrawing it. `Handover` keeps what is shown and what is
leaving, with a serial bumped on each change for the animation keys, and drops the leaving once
`HANDS_OVER` is out (`a_skip_keeps_the_last_song_beneath_until_the_handover_is_out`).

- **The words** are keyed by title, artist and album. The leaving song is drawn beneath at the same
  place under an id of its own and fades out by `LEAVING_GONE_BY` (40 %); the arriving song is drawn
  on top, keeps the presses, and fades in from `ARRIVING_FROM` (30 %), so the two are never both
  legible at once. Nothing slides.
- **The cover** is keyed by what it pictures. The leaving face stays opaque beneath and the arriving
  one — art or the disc — fades in over it, which is a true crossfade because a cover is opaque.
- **The signal path is held between songs.** The engine publishes no current track while the next
  row opens, and the path once dropped out for that moment; the text column is centred, so the
  title and by-line fell by half a line and rose again with the next song, which read as the text
  arriving from below.

## Scrollbars

Under `AutoHidden` a thumb fades in over `THUMB_ARRIVES_OVER` and out over the last
`THUMB_LEAVES_OVER` of `SCROLLED_LINGERS`, read in paint from when the run of scrolling began
(`Scrolled::since`) and when it last moved. The paint asks for frames only while the thumb is part
way, and the one timer a scroll starts wakes the window as the fade-out begins.

## Frames

An animation asks for the next frame of the view it is in, and the window is four cached `Part`s, so
a switch flipping redraws the pane and not the transport. Every motion here is a few hundred
milliseconds at most, and nothing asks for frames once it has landed.
