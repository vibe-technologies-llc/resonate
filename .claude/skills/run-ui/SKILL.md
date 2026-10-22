---
name: run-ui
description: Launch, screenshot and stop the Resonate GPUI front end on the user's live KWin Wayland session. Use whenever the app needs to be run, seen, or driven for real - the agent shell does not inherit the session environment, so a bare `cargo run` fails with "neither DISPLAY nor WAYLAND_DISPLAY is set".
---

# Running the Resonate UI

The agent shell has no `WAYLAND_DISPLAY`, `DISPLAY` or `DBUS_SESSION_BUS_ADDRESS`, so gpui refuses to
open a window. Supply them.

## Recover the session environment

```bash
SESSION_PID=$(pgrep -x plasmashell | head -1)
eval "$(tr '\0' '\n' < /proc/$SESSION_PID/environ |
        grep -E '^(WAYLAND_DISPLAY|DISPLAY|XDG_RUNTIME_DIR|XDG_SESSION_TYPE|DBUS_SESSION_BUS_ADDRESS)=' |
        sed 's/^/export /')"
```

Check `ls $XDG_RUNTIME_DIR/wayland-*`. `WAYLAND_DISPLAY` and `XDG_RUNTIME_DIR` open the window;
`DBUS_SESSION_BUS_ADDRESS` is needed for portals, screenshots included.

## Launch

The `ui` feature is default, so the plain binary is the UI. It also needs a PipeWire daemon (the
engine starts before the window opens). `--library` points at a scratch database filled by
`resonate scan <roots>`, never the user's own.

```bash
cargo build -p resonate
setsid env WAYLAND_DISPLAY=$WAYLAND_DISPLAY XDG_RUNTIME_DIR=$XDG_RUNTIME_DIR \
    RESONATE_LOG=warn,resonate=info \
    ./target/debug/resonate --library <db path> > /path/to/ui.log 2>&1 < /dev/null & disown
```

`setsid` and `disown` are load-bearing: a plain `&` dies when the tool call's shell returns. Allow a
second to map. The radv "not a conformant Vulkan implementation" warning is normal.

## With no desktop session

If `pgrep -x plasmashell` finds nothing (only the greeter's `kwin_wayland`, another user's), host
one: a virtual `kwin_wayland`, the app and `spectacle` inside it, under `dbus-run-session`. The
user's PipeWire in `/run/user/1000` still serves the engine.

```bash
export XDG_RUNTIME_DIR=/run/user/1000
kwin_wayland --virtual --socket=zz --width 1600 --height 900 --no-lockscreen &
sleep 5
WAYLAND_DISPLAY=zz ./target/debug/resonate --library <scratch db> &
sleep 20
WAYLAND_DISPLAY=zz spectacle -b -n -f -o /path/to/shot.png
pkill -x resonate
```

Run it as `dbus-run-session -- script.sh`. `pkill -x kwin_wayland` is refused for the greeter's
(harmless); kill your own by pid. Then check `pgrep -x resonate` is empty.

## The taskbar icon

The compositor matches `app_id` (`resonate`) to a desktop entry of that name for `Icon=`, so an
unpackaged tree shows the generic mark. Install the two files the PKGBUILD installs (copied, not
linked: repeat after either changes):

```bash
install -Dm644 packaging/resonate.desktop ~/.local/share/applications/resonate.desktop
install -Dm644 packaging/resonate.svg ~/.local/share/icons/hicolor/scalable/apps/resonate.svg
gtk-update-icon-cache -f -t ~/.local/share/icons/hicolor
update-desktop-database ~/.local/share/applications
kbuildsycoca6 --noincremental
```

The window rewrites that icon to take the accent and restores it at Resonate's own accent. To read
what it writes without touching the real one, run with `XDG_DATA_HOME` at a scratch folder and read
`icons/hicolor/scalable/apps/resonate.svg` there.

## Screenshot

No `grim` on KWin; `spectacle` needs the D-Bus address (`-b` background, `-n` no notification, `-f`
full screen):

```bash
env WAYLAND_DISPLAY=$WAYLAND_DISPLAY XDG_RUNTIME_DIR=$XDG_RUNTIME_DIR \
    DBUS_SESSION_BUS_ADDRESS=$DBUS_SESSION_BUS_ADDRESS \
    spectacle -b -n -f -o /path/to/shot.png
```

The screen is 4480x1440 and the window placement is unpredictable: downscale first
(`PIL.Image.thumbnail((1400, 1400))`), locate the window, crop the original.

## Stop

`pkill -x resonate` when finished: it is the user's own desktop. Never `pkill -f` (it matches the
calling shell and kills the tool call). Wait two seconds before relaunching, or the new window
becomes `org.mpris.MediaPlayer2.resonate.instance<pid>` and `busctl` calls to the plain name miss.

## Driving playback without a pointer

The window owns `org.mpris.MediaPlayer2.resonate`, so transport, queue, inspector and lyrics can be
filled from the shell. `AddTrack` with `NoTrack` as anchor inserts at the front: add in reverse or
`GoTo` the wanted row. `Play` on an idle queue does nothing; `GoTo` starts it.

```bash
uri="file://$(python3 -c 'import urllib.parse,sys; print(urllib.parse.quote(sys.argv[1]))' "$file")"
busctl --user call org.mpris.MediaPlayer2.resonate /org/mpris/MediaPlayer2 \
    org.mpris.MediaPlayer2.TrackList AddTrack sob "$uri" /org/mpris/MediaPlayer2/TrackList/NoTrack false
busctl --user get-property org.mpris.MediaPlayer2.resonate /org/mpris/MediaPlayer2 \
    org.mpris.MediaPlayer2.TrackList Tracks
busctl --user call org.mpris.MediaPlayer2.resonate /org/mpris/MediaPlayer2 \
    org.mpris.MediaPlayer2.TrackList GoTo o "$path"
```

A lyric sidecar is found by symlinking a track into a scratch folder and writing an `.lrc` beside the
link (the sidecar walks the folder of the queued path).

KWin focus-stealing prevention can leave a fresh window behind the terminal: ask the user to raise
it, or set `workspace.activeWindow = w` from a KWin script loaded via
`qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.loadScript`.

## What cannot be checked this way

A screenshot proves layout and paint, not interaction: the live session takes no synthetic input
without the remote-desktop portal, and there is no `ydotool`, `wtype`, `dotool` or `xdotool` (and
never `uinput`, which reaches the live pointer). A pointer gesture can be driven on a compositor of
its own: the `drive-a-drag` skill hosts a virtual KWin, drags a file onto the window and screenshots
the overlay. Anything else behind a click renders only in its default state, or through temporary
edits that reach what a click would:

- a pane: mark it `#[default]` on `Pane` (`crates/resonate-ui/src/views/root.rs`);
- a search: call `model.set_query(...)` beside the `model.reload` at the end of `LibraryModel::new`;
- a narrow window: shrink the `opening_size` that `bounds` in `app.rs` is centred on;
- a seam with nothing behind it: register a provider that answers.

Revert all before committing and check `git diff` for a leftover `#[default]`. Anything behind a
click or keystroke renders only in its default state: confirm with the user, or exercise the layer
beneath through `resonate play`, `resonate scan` and the library tests. Never report an interaction
as working from a screenshot.
