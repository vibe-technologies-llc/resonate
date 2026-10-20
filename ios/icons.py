import plistlib
from pathlib import Path
import re

ACCENTS = ("mauve", "blue", "teal", "green", "amber", "peach", "red")
PACKAGED_STOPS = ('stop-color="#6ea8fe"', 'stop-color="#63d29b"')
ICON_SIDES = (("", 1024), ("@2x", 120), ("@3x", 180))
PRIMARY_ICON = "Icon"
ICON_RESOURCE = re.compile(r"^\s*-\s*Generated/icons/(\S+\.png)\s*$", re.MULTILINE)


class Unreadable(Exception):
    pass


def found(pattern, text, what):
    matched = re.search(pattern, text)
    if matched is None:
        raise Unreadable(f"{what} is not where build-native.py reads it: /{pattern}/")
    return matched.group(1)


def resonate_accents(repository):
    theme = (repository / "crates/resonate-ui/src/theme.rs").read_text()
    start = theme.find("static RESONATE")
    if start < 0:
        raise Unreadable("the Resonate flavour is not in theme.rs")
    flavour = theme[start:]
    flavour = flavour[: flavour.index("});")]
    return {name: int(found(rf"{name}: 0x([0-9a-f]{{6}})", flavour, f"the {name} accent"), 16) for name in ACCENTS}


def lifted_toward_white(repository):
    launcher = (repository / "crates/resonate-ui/src/launcher.rs").read_text()
    return float(found(r"LIFTED_TOWARD_WHITE: f32 = ([0-9.]+);", launcher, "LIFTED_TOWARD_WHITE"))


def lifted(colour, toward_white):
    channels = [(colour >> shift) & 0xFF for shift in (16, 8, 0)]
    raised = [round((255 - value) * toward_white + value) for value in channels]
    return (raised[0] << 16) | (raised[1] << 8) | raised[2]


def recoloured(drawing, stops, colours):
    for stop, colour in zip(stops, colours):
        held = drawing.count(stop)
        if held != 1:
            raise Unreadable(f"packaging/resonate.svg holds {stop} {held} times, not once")
        drawing = drawing.replace(stop, f'stop-color="#{colour:06x}"')
    return drawing


def drawn(packaged, colour, toward_white):
    return recoloured(packaged, PACKAGED_STOPS, (lifted(colour, toward_white), colour))


def icon_names(accents):
    return {f"Icon{name.capitalize()}{suffix}.png" for name in accents for suffix, _ in ICON_SIDES}


def declared_in_xtool(repository):
    text = (repository / "ios/xtool.yml").read_text()
    return set(ICON_RESOURCE.findall(text))


def declared_in_info(repository):
    with (repository / "ios/Resonate/Info.plist").open("rb") as plist:
        icons = plistlib.load(plist)["CFBundleIcons"]
    primary = set(icons["CFBundlePrimaryIcon"]["CFBundleIconFiles"])
    alternates = {name: set(entry["CFBundleIconFiles"]) for name, entry in icons["CFBundleAlternateIcons"].items()}
    return primary, alternates


def check_declared(repository, generated):
    problems = []
    names = icon_names(ACCENTS)
    if generated != names:
        problems.append(f"drawn {sorted(generated)}, expected {sorted(names)}")
    in_xtool = declared_in_xtool(repository)
    if in_xtool != names:
        problems.append(f"xtool.yml lists {sorted(in_xtool ^ names)} differently from the icons drawn")
    primary, alternates = declared_in_info(repository)
    if primary != {PRIMARY_ICON}:
        problems.append(f"Info.plist's primary icon is {sorted(primary)}, not {PRIMARY_ICON}")
    wanted = {name.capitalize(): {f"Icon{name.capitalize()}"} for name in ACCENTS}
    if alternates != wanted:
        problems.append(f"Info.plist's alternate icons are {alternates}, not {wanted}")
    if problems:
        raise Unreadable("; ".join(problems))
