---
name: drive-a-drag
description: Drag a file from another Wayland window onto the Resonate window in a hosted virtual KWin, with real pointer events, and screenshot the drop overlay mid-drag. Use whenever drag-and-drop, the pointer or anything behind a click needs driving for real; the live session takes no synthetic input.
---

# Driving a drag on a hosted compositor

`gpui` cannot build an `ExternalPaths` in a test, so `Driven` calls `dragged_over` and `dropped`
directly (`ui.md`). This skill drives the platform's own events instead: a `kwin_wayland --virtual`
of its own, a GTK window offering a file as the drag source, the app as the target, and pointer
events from KWin's `org_kde_kwin_fake_input`.

```bash
.claude/skills/drive-a-drag/drag.sh [output folder]
```

It builds `resonate` (default features, the `ui` one) and `fakein`, hosts the compositor, places the
two windows (`place.js`, through KWin's scripting), presses on the source, drags to the window's
middle in 40 steps, holds, takes `over.png`, releases, waits and takes `after.png`. It exits 1 unless
the source saw `drag-end False` (the target took the copy), the file landed in the run's music
folder and the screenshot exists. Look at `over.png` (downscale it first: `PIL.Image.thumbnail`): it
shows the overlay, the destination and each name with its mark. The run's logs and scratch library
are in the output folder.

## What keeps it off the user's desktop

Every one of these was learnt from a leak:

- **Scratch `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME` and `XDG_STATE_HOME`.** A hosted
  `kwin_wayland` writes its output layout to `~/.config/kwinoutputconfig.json`, the live
  compositor's: a run without them left a `Virtual-0` output and setup in it.
- **A bus with nothing to activate** (`drag.sh` re-runs itself under `dbus-run-session
  --config-file=`, as `crates/resonate-mpris/tests/bus.rs` does). On the default one a screenshot
  activates `xdg-desktop-portal`, its KDE backend and `ksecretd`, which outlive the session and are
  reparented to the user's manager.
- **A socket of its own** (`resonate-drag`) beside the live `wayland-1`, and the
  `KWIN_WAYLAND_NO_PERMISSION_CHECKS` that lets a client bind `org_kde_kwin_fake_input` set for the
  hosted compositor alone. Never `uinput` (`/dev/uinput` is writable by the user, and what it makes
  reaches the live session's pointer), never the live compositor's fake input.
- **Kill by pid**, `kill $PIDS` in a trap, then `kill -9`: `pkill -f` matches the calling shell and
  kills the tool call, and `pkill -x pipewire` kills the user's own daemon.

A run's output goes to a file when called from a tool that waits on its pipe: a service the bus
started can hold it open after the script ends.

## The parts

- `fakein/`: a small Wayland client over `wayland-protocols-plasma`, reading `move X Y`, `down`,
  `up`, `rdown`, `rup`, `sleep S`, `drag X0 Y0 X1 Y1 STEPS` and `quit` from its stdin and answering `fakein: done
  <line>` for each, which `drag.sh` waits on: commands sent ahead of the pointer take their
  screenshot before the drag has moved. It binds version 5 of the global, the newest the crate knows
  (KWin offers 6). It is its own Cargo project (`[workspace]` in its manifest), built into
  `target/drive-a-drag`.
- `source.py`: a GTK 4 window, `dragsource`, whose label is a `Gtk.DragSource` offering its
  arguments as a `Gdk.FileList` (`text/uri-list`), logging `prepare`, `drag-begin`, `drag-end` and
  `drag-cancel` (reason 2 is no target). KWin's EIS (`org.kde.KWin.EIS.RemoteDesktop`) was tried
  first and disconnected the client every time.
- `place.js`: sets the geometry of the windows captioned `dragsource` and `Resonate`, so the drag's
  start and end points are known (the source at 40,40, the app at 320,60).

To drive another gesture, change the command list in `drag.sh`; a click is `down` then `up`, and a
window's contents are only where the screenshot shows them.

## What it proves

That a file dragged from another client reaches the root's `on_drag_move::<ExternalPaths>` on a real
Wayland compositor (KWin), draws the overlay, and that the drop copies the file. `crates/resonate/tests/dragging.rs` runs it with `RESONATE_DRAG_TESTS=1` and checks what it checks; it is not in CI
(a compositor, a GPU and a Vulkan driver), and it says nothing of another compositor's data-device
behaviour.
