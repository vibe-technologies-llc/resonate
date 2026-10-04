---
name: run-ui
description: Launch, screenshot and stop the Resonate GPUI front end on the user's live KWin Wayland session. Use whenever the app needs to be run, seen, or driven for real - the agent shell does not inherit the session environment, so a bare `cargo run` fails with "neither DISPLAY nor WAYLAND_DISPLAY is set".
---

# Running the Resonate UI

The agent shell starts with no `WAYLAND_DISPLAY`, `DISPLAY` or `DBUS_SESSION_BUS_ADDRESS`, so gpui
refuses to open a window. The compositor is fine; the variables have to be supplied.

## Recover the session environment

Read it from a process already inside the session:

```bash
SESSION_PID=$(pgrep -x plasmashell | head -1)
eval "$(tr '\0' '\n' < /proc/$SESSION_PID/environ |
        grep -E '^(WAYLAND_DISPLAY|DISPLAY|XDG_RUNTIME_DIR|XDG_SESSION_TYPE|DBUS_SESSION_BUS_ADDRESS)=' |
        sed 's/^/export /')"
```

Confirm the socket exists with `ls $XDG_RUNTIME_DIR/wayland-*` before blaming the app.
`WAYLAND_DISPLAY` and `XDG_RUNTIME_DIR` open the window; `DBUS_SESSION_BUS_ADDRESS` is needed for
anything through a portal, a screenshot included.

## Launch

The `ui` feature is on by default, so the plain binary is the UI. It needs a PipeWire daemon as well
as the compositor, because `Player::new` starts the engine before the window opens. Point
`--library` at a scratch database, populated with `resonate scan <roots>`, never the user's own.

```bash
cargo build -p resonate
setsid env WAYLAND_DISPLAY=$WAYLAND_DISPLAY XDG_RUNTIME_DIR=$XDG_RUNTIME_DIR \
    RESONATE_LOG=warn,resonate=info \
    ./target/debug/resonate --library <db path> > /path/to/ui.log 2>&1 < /dev/null & disown
```

`setsid` and `disown` are load-bearing: a plain `&` leaves the window in the tool call's shell and it
dies when the call returns. Give it a second to map. The stderr warning that radv is not a conformant
Vulkan implementation is normal.

## The taskbar icon

The compositor matches the window's `app_id` (`resonate`) against a desktop entry of that name and
takes `Icon=` from it, so a tree that has never been packaged shows the generic mark. Install the two
files the PKGBUILD installs, and again after either changes (they are copied, not linked):

```bash
install -Dm644 packaging/resonate.desktop ~/.local/share/applications/resonate.desktop
install -Dm644 packaging/resonate.svg ~/.local/share/icons/hicolor/scalable/apps/resonate.svg
gtk-update-icon-cache -f -t ~/.local/share/icons/hicolor
update-desktop-database ~/.local/share/applications
kbuildsycoca6 --noincremental
```

A copy of the packaged icon standing there takes the accent and is put back when the accent is
Resonate's own again. To read what the window writes without touching it, run with `XDG_DATA_HOME`
at a scratch folder and read `icons/hicolor/scalable/apps/resonate.svg` under it.

## Screenshot

KWin has no `grim`; use `spectacle`, which needs the D-Bus address. `-b` background, `-n` no
notification, `-f` full screen:

```bash
env WAYLAND_DISPLAY=$WAYLAND_DISPLAY XDG_RUNTIME_DIR=$XDG_RUNTIME_DIR \
    DBUS_SESSION_BUS_ADDRESS=$DBUS_SESSION_BUS_ADDRESS \
    spectacle -b -n -f -o /path/to/shot.png
```

The screen is 4480x1440 and the window is not placed predictably: downscale the shot first
(`PIL.Image.thumbnail((1400, 1400))`), read it to locate the window, then crop the original to it.

## Stop

`pkill -x resonate`, always when finished: it is the user's own desktop. Use `-x`; `pkill -f` matches
the shell running the `pkill` itself and kills the tool call. Wait two seconds before relaunching, or
the new window falls back to `org.mpris.MediaPlayer2.resonate.instance<pid>` and every `busctl` call
to the plain name misses.

## Driving playback without a pointer

The window puts `org.mpris.MediaPlayer2.resonate` on the bus, so the transport, queue, inspector and
lyrics pane can be filled from the shell. `AddTrack` with `NoTrack` as the anchor inserts at the
*front*, so add in reverse or `GoTo` the row wanted; `Play` on an idle queue does nothing, `GoTo`
starts it:

```bash
uri="file://$(python3 -c 'import urllib.parse,sys; print(urllib.parse.quote(sys.argv[1]))' "$file")"
busctl --user call org.mpris.MediaPlayer2.resonate /org/mpris/MediaPlayer2 \
    org.mpris.MediaPlayer2.TrackList AddTrack sob "$uri" /org/mpris/MediaPlayer2/TrackList/NoTrack false
busctl --user get-property org.mpris.MediaPlayer2.resonate /org/mpris/MediaPlayer2 \
    org.mpris.MediaPlayer2.TrackList Tracks
busctl --user call org.mpris.MediaPlayer2.resonate /org/mpris/MediaPlayer2 \
    org.mpris.MediaPlayer2.TrackList GoTo o "$path"
```

A lyric sidecar is seen by symlinking a track into a scratch folder and writing an `.lrc` beside the
link, because the sidecar walks the folder of the path the queue holds.

KWin's focus-stealing prevention can leave a fresh window behind the terminal; ask the user to raise
it, or activate it with `workspace.activeWindow = w` from a KWin script loaded through
`qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.loadScript`.

## What cannot be checked this way

A screenshot proves layout and paint, not interaction: KWin exposes no synthetic input without the
remote-desktop portal, and no `ydotool`, `wtype`, `dotool` or `xdotool` is installed. Temporary edits
reach states a click would:

- a pane: mark it `#[default]` on `Pane` (`crates/resonate-ui/src/views/root.rs`) and rebuild;
- a search: call `model.set_query(...)` beside the `model.reload` at the end of `LibraryModel::new`;
- a narrow window: shrink the `opening_size` the `bounds` in `app.rs` are centred on;
- a seam with nothing behind it: register a provider that answers.

Revert them all before committing and check `git diff` for a leftover `#[default]`. Anything behind a
click or keystroke (pane switcher, seek bar, device picker, search field) renders only in its default
state here: confirm with the user, or exercise the layer underneath through `resonate play`,
`resonate scan` and the library tests. Never report an interaction as working on a screenshot.
