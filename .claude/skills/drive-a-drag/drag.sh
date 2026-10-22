#!/bin/bash
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../../.." && pwd)
OUT=${1:-$(mktemp -d)}
SOCKET=resonate-drag
WIDTH=1600
HEIGHT=900
FROM="100 100"
TO="800 450"

mkdir -p "$OUT/music" "$OUT/files"

if [ -z "${DRAG_ON_A_BUS_OF_ITS_OWN:-}" ]; then
    cat > "$OUT/session.conf" <<'CONF'
<!DOCTYPE busconfig PUBLIC
 "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:tmpdir=/tmp</listen>
  <policy context="default">
    <allow send_destination="*"/>
    <allow receive_sender="*"/>
    <allow own="*"/>
  </policy>
</busconfig>
CONF
    DRAG_ON_A_BUS_OF_ITS_OWN=1 exec dbus-run-session --config-file="$OUT/session.conf" -- "$0" "$OUT"
fi

echo song > "$OUT/files/one.flac"
rm -f "$OUT/music/one.flac" "$OUT/cmd" "$OUT/fakein.log"
cargo build -p resonate --manifest-path "$REPO/Cargo.toml" || exit 1
cargo build --manifest-path "$HERE/fakein/Cargo.toml" --target-dir "$REPO/target/drive-a-drag" || exit 1
FAKEIN="$REPO/target/drive-a-drag/debug/fakein"
printf 'online = false\nsync = false\nmusic-folder = "%s"\n' "$OUT/music" > "$OUT/config.toml"

export XDG_RUNTIME_DIR=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
export XDG_CONFIG_HOME="$OUT/xdg/config" XDG_DATA_HOME="$OUT/xdg/data"
export XDG_CACHE_HOME="$OUT/xdg/cache" XDG_STATE_HOME="$OUT/xdg/state"
mkdir -p "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_CACHE_HOME" "$XDG_STATE_HOME"
unset WAYLAND_DISPLAY DISPLAY

KWIN_WAYLAND_NO_PERMISSION_CHECKS=1 kwin_wayland --virtual --socket=$SOCKET \
    --width $WIDTH --height $HEIGHT --no-lockscreen > "$OUT/kwin.log" 2>&1 &
PIDS="$!"
trap 'kill $PIDS 2>/dev/null; sleep 1; kill -9 $PIDS 2>/dev/null' EXIT
sleep 4

export WAYLAND_DISPLAY=$SOCKET GDK_BACKEND=wayland
python3 -I "$HERE/source.py" "$OUT/files/one.flac" > "$OUT/source.log" 2>&1 &
PIDS="$PIDS $!"
RESONATE_LOG=info "$REPO/target/debug/resonate" --config "$OUT/config.toml" \
    --library "$OUT/library.db" > "$OUT/resonate.log" 2>&1 &
PIDS="$PIDS $!"
sleep 12

script=$(busctl --user call org.kde.KWin /Scripting org.kde.kwin.Scripting loadScript ss "$HERE/place.js" place | awk '{print $2}')
busctl --user call org.kde.KWin "/Scripting/Script$script" org.kde.kwin.Script run > /dev/null
sleep 2

mkfifo "$OUT/cmd"
"$FAKEIN" < "$OUT/cmd" > "$OUT/fakein.log" 2>&1 &
PIDS="$PIDS $!"
exec 3> "$OUT/cmd"
sent=0
send() {
    echo "$1" >&3
    sent=$((sent + 1))
    until [ "$(grep -c '^fakein: done' "$OUT/fakein.log")" -ge "$sent" ]; do sleep 0.1; done
}
until grep -q '^fakein: ready' "$OUT/fakein.log"; do sleep 0.1; done

set -- $FROM
send "move $1 $2"
send "sleep 0.3"
send "down"
send "sleep 0.3"
send "drag $FROM $TO 40"
send "sleep 1.5"
spectacle -b -n -f -o "$OUT/over.png" > /dev/null 2>&1
send "up"
send "sleep 3"
spectacle -b -n -f -o "$OUT/after.png" > /dev/null 2>&1
echo quit >&3

status=0
grep -q '^source: drag-end False' "$OUT/source.log" || { echo "the drag was not accepted"; status=1; }
[ -f "$OUT/music/one.flac" ] || { echo "the dropped file was not copied into the music folder"; status=1; }
[ -s "$OUT/over.png" ] || { echo "no screenshot of the drag"; status=1; }
echo "screenshots and logs in $OUT"
exit $status
