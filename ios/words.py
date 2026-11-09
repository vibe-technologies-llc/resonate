from dataclasses import dataclass
from pathlib import Path
import plistlib
import re

ios = Path(__file__).resolve().parent
sources = ios / "Sources"
english = ios / "Resonate/en.lproj"

SAID_FOR_THE_LOG = (
    "log.",
    "log(",
    "Logger(",
    "print(",
    "fatalError(",
    "precondition(",
    "preconditionFailure(",
    "assert(",
    "assertionFailure(",
)
KEYED_BY = ("String(localized:", "Text(", "LocalizedStringKey(", "LocalizedStringResource(")
KEY = re.compile(r"^[a-z][a-z0-9-]*(\.[a-z0-9-]+)+$")
ENTRY = re.compile(r'^"((?:[^"\\]|\\.)*)"\s*=\s*"((?:[^"\\]|\\.)*)";$')


@dataclass(frozen=True)
class Literal:
    line: int
    text: str
    keyed: bool


def reads_as_prose(text):
    words = [word for word in re.split(r"[^\w']+", text) if sum(letter.isalpha() for letter in word) >= 2]
    words = [word for word in words if not word.isdigit()]
    capitalised = any(
        len(letters) >= 2 and letters[0].isupper() and letters[1].islower()
        for letters in ("".join(letter for letter in word if letter.isalpha()) for word in words)
    )
    spaced = any(letter.isspace() for letter in text) and bool(words)
    return capitalised or spaced


class Lexer:
    def __init__(self, source):
        self.source = source
        self.at = 0
        self.line = 1
        self.depth = 0
        self.quiet = []
        self.keying = []
        self.found = []

    def peek(self, offset=0):
        place = self.at + offset
        return self.source[place] if place < len(self.source) else ""

    def starts(self, text):
        return self.source.startswith(text, self.at)

    def step(self):
        letter = self.source[self.at]
        self.at += 1
        if letter == "\n":
            self.line += 1
        return letter

    def literals(self):
        self.code(stop_at_paren=False)
        return self.found

    def leave(self, stack):
        left = False
        while stack and stack[-1] >= self.depth:
            stack.pop()
            left = True
        return left

    def code(self, stop_at_paren):
        parens = 0
        while self.at < len(self.source):
            letter = self.peek()
            if self.starts("//"):
                while self.at < len(self.source) and self.peek() != "\n":
                    self.step()
            elif self.starts("/*"):
                while self.at < len(self.source) and not self.starts("*/"):
                    self.step()
                self.at = min(len(self.source), self.at + 2)
            elif self.starts('#"""') or self.starts('"""'):
                self.string(raw=self.peek() == "#", multiline=True)
            elif self.starts('#"'):
                self.string(raw=True, multiline=False)
            elif letter == '"':
                self.string(raw=False, multiline=False)
            elif letter in "({[":
                self.step()
                self.depth += 1
                parens += letter == "("
            elif letter in ")}]":
                if stop_at_paren and letter == ")" and parens == 0:
                    self.step()
                    return
                self.step()
                self.depth -= 1
                parens -= letter == ")"
                if self.leave(self.quiet) and letter == ")":
                    ahead = self.at
                    while ahead < len(self.source) and self.source[ahead] == " ":
                        ahead += 1
                    if ahead < len(self.source) and self.source[ahead] == "{":
                        self.quiet.append(self.depth)
                self.leave(self.keying)
            elif any(self.starts(marker) and not self.named_before(marker) for marker in SAID_FOR_THE_LOG):
                marker = next(marker for marker in SAID_FOR_THE_LOG if self.starts(marker))
                for _ in range(len(marker) - 1):
                    self.step()
                self.quiet.append(self.depth)
            elif any(self.starts(marker) and not self.named_before(marker) for marker in KEYED_BY):
                marker = next(marker for marker in KEYED_BY if self.starts(marker))
                for _ in range(marker.index("(")):
                    self.step()
                self.keying.append(self.depth)
            else:
                self.step()

    def named_before(self, marker):
        before = self.source[self.at - 1] if self.at else ""
        return before.isalnum() or before == "_" or (marker[0].islower() and before == "")

    def string(self, raw, multiline):
        started = self.line
        hashes = 0
        while self.peek() == "#":
            self.step()
            hashes += 1
        quotes = 3 if multiline else 1
        for _ in range(quotes):
            self.step()
        closing = '"' * quotes + "#" * hashes
        escape = "\\" + "#" * hashes
        text = []
        while self.at < len(self.source) and not self.starts(closing):
            if self.starts(escape + "("):
                for _ in range(len(escape) + 1):
                    self.step()
                text.append("{}")
                self.code(stop_at_paren=True)
            elif self.starts(escape):
                for _ in range(len(escape)):
                    self.step()
                escaped = self.step()
                text.append(" " if escaped == "n" else escaped)
            else:
                text.append(self.step())
        self.at = min(len(self.source), self.at + len(closing))
        if not self.quiet:
            self.found.append(Literal(started, "".join(text), bool(self.keying)))


def literals_of(source):
    return Lexer(source).literals()


def prose_of(source):
    return [literal for literal in literals_of(source) if reads_as_prose(literal.text)]


def keys_used_in(source):
    return {literal.text for literal in literals_of(source) if literal.keyed and KEY.match(literal.text)}


def strings_file(path):
    entries = {}
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip():
            continue
        found = ENTRY.match(line.strip())
        if not found:
            raise ValueError(f"{path.name}:{number} is not a \"key\" = \"value\"; line")
        if found.group(1) in entries:
            raise ValueError(f"{path.name}:{number} names {found.group(1)} twice")
        entries[found.group(1)] = found.group(2).replace('\\"', '"').replace("\\\\", "\\")
    return entries


def plurals():
    with (english / "Localizable.stringsdict").open("rb") as plist:
        return plistlib.load(plist)


def swift_files():
    return sorted(sources.rglob("*.swift"))
