import re

STYLE = re.compile(r"\x1b\[[0-9;]*m|\x1b\]8;;[^\x1b]*\x1b\\")
WARNING = re.compile(r"^(?:\S.*?:\d+:\d+: |ld: )?warning: ")
COMPLETE = re.compile(r"^Build complete!", re.MULTILINE)


def plain(output):
    return STYLE.sub("", output)


def warnings_in(output):
    return [line for line in plain(output).splitlines() if WARNING.search(line)]


def completed(output):
    return COMPLETE.search(plain(output)) is not None
