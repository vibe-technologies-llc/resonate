from collections import Counter
import json
from pathlib import Path
import re

SOURCES = Path(__file__).resolve().parent / "Sources" / "Resonate"
BASELINE = Path(__file__).resolve().parent / "accessibility-baseline.json"

SYMBOL_NAMED_WITHIN = 12
GESTURE_ANSWERED_WITHIN = 14
ANIMATION_GATED_WITHIN = 30
PROGRESS_LABELLED_WITHIN = 3

SYMBOL = re.compile(r"\bImage\(systemName:")
GESTURE = re.compile(r"\.onTapGesture\b|\bDragGesture\(|\bSpatialTapGesture\(|\bLongPressGesture\(")
ANIMATION = re.compile(r"\bwithAnimation\(|\.animation\(")
PROGRESS = re.compile(r"\bProgressView\(")
FIXED_FONT = re.compile(r"\.system\(size:\s*\d")

NAMES_A_SYMBOL = re.compile(r"accessibilityLabel|accessibilityHidden|accessibilityElement\(children: \.ignore\)")
ANSWERS_A_GESTURE = re.compile(r"accessibilityAction|accessibilityAddTraits|accessibilityAdjustableAction|accessibilityActivate|isModal")
GATES_AN_ANIMATION = re.compile(r"reduceMotion|accessibilityReduceMotion|withAnimation\(nil|\.animation\(nil")
LABELS_A_PROGRESS = re.compile(r"accessibilityLabel|accessibilityHidden|ProgressView\(\"|ProgressView\([^)]*\)\s*\{|\{ Text\(")
BARE_PROGRESS = re.compile(r"ProgressView\(\s*(\)|value:|timerInterval:)")


def window(lines, at, before, after):
    return "\n".join(lines[max(at - before, 0) : at + after + 1])


def findings_in(text):
    lines = text.splitlines()
    found = []
    for at, line in enumerate(lines):
        stripped = line.strip()
        if not stripped:
            continue
        if SYMBOL.search(line) and not NAMES_A_SYMBOL.search(window(lines, at, 0, SYMBOL_NAMED_WITHIN)):
            found.append(("unlabelled symbol", stripped))
        if GESTURE.search(line) and not ANSWERS_A_GESTURE.search(window(lines, at, GESTURE_ANSWERED_WITHIN, GESTURE_ANSWERED_WITHIN)):
            found.append(("gesture without an action or a trait", stripped))
        if ANIMATION.search(line) and not GATES_AN_ANIMATION.search(window(lines, at, ANIMATION_GATED_WITHIN, 2)):
            found.append(("animation without a reduce motion gate", stripped))
        if PROGRESS.search(line) and BARE_PROGRESS.search(line) and not LABELS_A_PROGRESS.search(window(lines, at, 0, PROGRESS_LABELLED_WITHIN)):
            found.append(("progress view without a label", stripped))
        if FIXED_FONT.search(line):
            found.append(("font of a fixed size", stripped))
    return found


def findings():
    keyed = Counter()
    for path in sorted(SOURCES.glob("*.swift")):
        for rule, stripped in findings_in(path.read_text()):
            keyed[(path.name, rule, stripped)] += 1
    return keyed


def baseline():
    if not BASELINE.exists():
        return Counter()
    kept = json.loads(BASELINE.read_text())
    return Counter({(entry["file"], entry["rule"], entry["line"]): entry["count"] for entry in kept})


def write_baseline(keyed):
    entries = [
        {"file": file, "rule": rule, "line": line, "count": count}
        for (file, rule, line), count in sorted(keyed.items())
    ]
    BASELINE.write_text(json.dumps(entries, indent=2, ensure_ascii=False) + "\n")


if __name__ == "__main__":
    write_baseline(findings())
