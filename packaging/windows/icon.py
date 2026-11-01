import struct
import subprocess
import sys
from pathlib import Path

SIDES = (16, 20, 24, 32, 40, 48, 64, 80, 96, 256)
LARGEST_SIDE = 256
DIRECTORY_HEADER = struct.Struct("<HHH")
DIRECTORY_ENTRY = struct.Struct("<BBBBHHII")
ICON_KIND = 1
BITS_PER_PIXEL = 32


def rendered(drawing, side):
    return subprocess.run(
        ["rsvg-convert", "--width", str(side), "--height", str(side), "--format", "png", str(drawing)],
        check=True,
        capture_output=True,
    ).stdout


def entry_side(side):
    return 0 if side == LARGEST_SIDE else side


def icon_of(images):
    first_offset = DIRECTORY_HEADER.size + DIRECTORY_ENTRY.size * len(images)
    directory = DIRECTORY_HEADER.pack(0, ICON_KIND, len(images))
    body = b""
    for side, png in images:
        offset = first_offset + len(body)
        directory += DIRECTORY_ENTRY.pack(entry_side(side), entry_side(side), 0, 0, 1, BITS_PER_PIXEL, len(png), offset)
        body += png
    return directory + body


def main():
    here = Path(__file__).resolve().parent
    drawing = here.parent / "resonate.svg"
    target = here / "resonate.ico"
    target.write_bytes(icon_of([(side, rendered(drawing, side)) for side in SIDES]))
    print(target)


if __name__ == "__main__":
    sys.exit(main())
