import struct
import subprocess
import sys
from pathlib import Path

CANVAS = 1024
PLATE = 824
DRAWN_ON = 128
ENTRIES = (
    (b"ic11", 32),
    (b"ic12", 64),
    (b"ic07", 128),
    (b"ic13", 256),
    (b"ic08", 256),
    (b"ic14", 512),
    (b"ic09", 512),
    (b"ic10", 1024),
)
MAGIC = b"icns"
HEADER = struct.Struct(">4sI")


def inset(drawing):
    margin = (CANVAS - PLATE) / 2
    scale = PLATE / DRAWN_ON
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {CANVAS} {CANVAS}" '
        f'width="{CANVAS}" height="{CANVAS}">'
        f'<g transform="translate({margin} {margin}) scale({scale})">{drawing}</g>'
        "</svg>"
    )


def rendered(drawing, side):
    return subprocess.run(
        ["rsvg-convert", "--width", str(side), "--height", str(side), "--format", "png"],
        input=drawing.encode(),
        check=True,
        capture_output=True,
    ).stdout


def icon_of(images):
    body = b"".join(HEADER.pack(kind, HEADER.size + len(png)) + png for kind, png in images)
    return HEADER.pack(MAGIC, HEADER.size + len(body)) + body


def main():
    here = Path(__file__).resolve().parent
    drawing = inset((here.parent / "resonate.svg").read_text())
    target = here / "resonate.icns"
    sides = {side: rendered(drawing, side) for side in sorted({side for _, side in ENTRIES})}
    target.write_bytes(icon_of([(kind, sides[side]) for kind, side in ENTRIES]))
    print(target)


if __name__ == "__main__":
    sys.exit(main())
