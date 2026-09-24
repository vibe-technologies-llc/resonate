---
name: run-ui
description: Launch, screenshot and stop the Resonate GPUI front end on the user's live KWin Wayland session. Use whenever the app needs to be run, seen, or driven for real - the agent shell does not inherit the session environment, so a bare `cargo run` fails with "neither DISPLAY nor WAYLAND_DISPLAY is set".
---

# Running the Resonate UI

The agent shell starts with no `WAYLAND_DISPLAY`, `DISPLAY` or `DBUS_SESSION_BUS_ADDRESS`, so gpui
refuses to open a window and reports *"neither DISPLAY nor WAYLAND_DISPLAY is set. You can run in
headless mode"*. There is nothing wrong with the compositor; the variables just have to be supplied.

## Recover the session environment

Read it from a process already inside the session rather than hardcoding it:

```bash
SESSION_PID=$(pgrep -x plasmashell | head -1)
eval "$(tr '\0' '\n' < /proc/$SESSION_PID/environ |
        grep -E '^(WAYLAND_DISPLAY|DISPLAY|XDG_RUNTIME_DIR|XDG_SESSION_TYPE|DBUS_SESSION_BUS_ADDRESS)=' |
        sed 's/^/export /')"
```

On this machine that resolves to `WAYLAND_DISPLAY=wayland-0`, `DISPLAY=:0`,
`XDG_RUNTIME_DIR=/run/user/1000`, `XDG_SESSION_TYPE=wayland`,
`DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus`. Confirm the socket exists with
`ls $XDG_RUNTIME_DIR/wayland-*` before blaming the app.

`WAYLAND_DISPLAY` and `XDG_RUNTIME_DIR` are enough to open the window. `DBUS_SESSION_BUS_ADDRESS` is
only needed for anything that goes through a portal, which includes taking a screenshot.

## Launch

The `ui` feature is on by default, so the plain binary is the UI:

```bash
cargo build -p resonate
setsid env WAYLAND_DISPLAY=$WAYLAND_DISPLAY XDG_RUNTIME_DIR=$XDG_RUNTIME_DIR \
    RESONATE_LOG=warn,resonate=info \
    ./target/debug/resonate --library <db path> > /path/to/ui.log 2>&1 < /dev/null & disown
```

`setsid` and `disown` are load-bearing: a plain `&` leaves the window in the tool call's own shell,
so it dies the moment that call returns and `pgrep -x resonate` finds nothing. Give it a second to
map. It needs a PipeWire daemon as well as the compositor, because `Player::new` starts the engine
before the window opens.

`WARNING: radv is not a conformant Vulkan implementation, testing use only` on stderr is normal and
not a failure.

Point `--library` at a scratch database rather than the user's real one. `resonate scan <roots>`
populates it, which is quicker than the Settings pane's own folder picker and needs no portal.

## The taskbar icon

The compositor matches the window's `app_id` — `resonate` — against a desktop entry of that name
and takes `Icon=` from it, so a tree that has never been packaged shows the generic Wayland mark.
gpui binds no `xdg-toplevel-icon-v1`, so there is no answer in the code. Install the two files the
PKGBUILD installs:

```bash
install -Dm644 packaging/resonate.desktop ~/.local/share/applications/resonate.desktop
install -Dm644 packaging/resonate.svg ~/.local/share/icons/hicolor/scalable/apps/resonate.svg
gtk-update-icon-cache -f -t ~/.local/share/icons/hicolor
update-desktop-database ~/.local/share/applications
kbuildsycoca6 --noincremental
```

They are installed on this machine already. Reinstall after either file changes — the entry is
copied, not linked, so an edit in `packaging/` does not reach the launcher on its own. A copy of
the packaged icon standing there is one the window did not draw, so the accent is never written
over it; to see the icon follow the accent, run with `XDG_DATA_HOME` pointed at a scratch folder
and read `icons/hicolor/scalable/apps/resonate.svg` under it.

## Screenshot

KWin has no `grim`; use `spectacle`, which needs the D-Bus address:

```bash
env WAYLAND_DISPLAY=$WAYLAND_DISPLAY XDG_RUNTIME_DIR=$XDG_RUNTIME_DIR \
    DBUS_SESSION_BUS_ADDRESS=$DBUS_SESSION_BUS_ADDRESS \
    spectacle -b -n -f -o /path/to/shot.png
```

`-b` background, `-n` no notification, `-f` full screen. The screen is 4480x1440, so downscale
before reading the image or it wastes most of the budget:

```bash
python3 -c "
from PIL import Image
im = Image.open('shot.png'); im.thumbnail((1400, 1400)); im.save('shot-small.png')"
```

To see the window rather than the whole desktop, crop to it instead of downscaling the lot.

## Stop

```bash
pkill -x resonate
```

Use `-x`, which matches the process name. `pkill -f target/debug/resonate` matches the full command
line of the shell running the `pkill` itself, so it kills the tool call and returns exit 144 before
reaching the app.

Always stop it when finished. It is the user's own desktop, and a leftover window sits on top of
their session.

## Finding the window in the shot

The window is not placed predictably and the shot is the whole desktop, so crop by looking rather
than by a remembered offset: downscale the full screenshot first, read it to locate the window, then
crop the original to those coordinates and read that.

## Driving playback without a pointer

The window puts `org.mpris.MediaPlayer2.resonate` on the bus, so the transport, the queue, the
inspector and the lyrics pane can all be filled from the shell. `AddTrack` with `NoTrack` as the
anchor inserts at the *front* of the queue, so add in reverse or `GoTo` the row wanted:

```bash
uri="file://$(python3 -c 'import urllib.parse,sys; print(urllib.parse.quote(sys.argv[1]))' "$file")"
busctl --user call org.mpris.MediaPlayer2.resonate /org/mpris/MediaPlayer2 \
    org.mpris.MediaPlayer2.TrackList AddTrack sob "$uri" /org/mpris/MediaPlayer2/TrackList/NoTrack false
busctl --user get-property org.mpris.MediaPlayer2.resonate /org/mpris/MediaPlayer2 \
    org.mpris.MediaPlayer2.TrackList Tracks          # the object paths, first row first
busctl --user call org.mpris.MediaPlayer2.resonate /org/mpris/MediaPlayer2 \
    org.mpris.MediaPlayer2.TrackList GoTo o "$path"  # loads and plays that row
```

`Play` on an idle queue does nothing; `GoTo` is what starts it. A lyric sidecar is seen by
symlinking a track into a scratch folder and writing an `.lrc` beside the link, because the
sidecar walks the folder of the path the queue holds.

Wait two seconds after `pkill -x resonate` before launching again: the old process releases the
bus name on exit, and a window started before it has falls back to
`org.mpris.MediaPlayer2.resonate.instance<pid>`, which every `busctl` call to the plain name then
misses with "The name is not activatable".

KWin's focus-stealing prevention can leave a freshly mapped window behind the terminal, and the
`output` property of a KWin script's window object is read-only; ask the user to raise it, or move
it with `w.frameGeometry` and activate it with `workspace.activeWindow = w` from a script loaded
through `qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.loadScript`.

## What cannot be checked this way

A screenshot proves layout and paint. It cannot drive the app. KWin exposes no synthetic input
without the remote-desktop portal, and none of `ydotool`, `wtype`, `dotool` or `xdotool` is installed
here - `xdotool` would reach only Xwayland clients anyway, and this is a native Wayland window.

A pane the sidebar has to be clicked to reach is seen by temporarily marking it `#[default]` on
`Pane` and rebuilding; a search is seen by adding a `model.set_query(...)` beside the `model.reload`
at the end of `LibraryModel::new`, which is what puts the *Reads* row and a narrowed pane's empty
message on screen; a narrow window is seen by shrinking the `size` the `bounds` in `app.rs` are
centred on; and a seam with nothing behind it is seen by temporarily registering a provider that
answers. Revert them all before committing - check `git diff` for the `#[default]` marker, which is
easy to leave behind.

So anything behind a click or a keystroke - the pane switcher, the seek bar, the device picker, the
search field - renders only in its default state. Confirm those with the user, or exercise the layer
underneath through `resonate play`, `resonate scan` and the library tests. Never report an
interaction as working on the strength of a screenshot.
