#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
"""Writes the tables of crates/core/src/linedraw.rs from the Unicode character names, or checks them.

    tools/box_drawing.py           rewrite the tables
    tools/box_drawing.py --check   fail if they are out of date

A name such as BOX DRAWINGS DOWN SINGLE AND RIGHT DOUBLE says which arms a character
has and how strong they are; QUADRANT UPPER LEFT AND LOWER RIGHT says which quarters a
block fills. Dashes, arcs and diagonals have no table entry: the font draws them.
"""

import sys
import unicodedata
from pathlib import Path

TARGET = Path(__file__).resolve().parent.parent / "crates" / "core" / "src" / "linedraw.rs"
BEGIN = "// BEGIN tables written by tools/box_drawing.py\n"
END = "// END tables written by tools/box_drawing.py\n"

DIRECTIONS = {
    "UP": ["UP"],
    "DOWN": ["DOWN"],
    "LEFT": ["LEFT"],
    "RIGHT": ["RIGHT"],
    "VERTICAL": ["UP", "DOWN"],
    "HORIZONTAL": ["LEFT", "RIGHT"],
}
STRENGTHS = {"LIGHT": 1, "SINGLE": 1, "HEAVY": 2, "DOUBLE": 3}
ORDER = ["UP", "RIGHT", "DOWN", "LEFT"]
QUARTERS = {"UPPER LEFT": 1, "UPPER RIGHT": 2, "LOWER LEFT": 4, "LOWER RIGHT": 8}


def arms(name):
    """[up, right, down, left] with 0 none, 1 light, 2 heavy, 3 double, or None."""
    name = name.removeprefix("BOX DRAWINGS ")
    if any(word in name.split() for word in ("DASH", "ARC", "DIAGONAL")):
        return None
    result = dict.fromkeys(ORDER, 0)
    strength = None
    for part in name.split(" AND "):
        words = part.split()
        if words[0] in STRENGTHS:
            strength, words = STRENGTHS[words[0]], words[1:]
        elif words[-1] in STRENGTHS:
            strength, words = STRENGTHS[words[-1]], words[:-1]
        if strength is None or not words or any(word not in DIRECTIONS for word in words):
            raise ValueError(f"cannot read {name!r}")
        for word in words:
            for direction in DIRECTIONS[word]:
                result[direction] = strength
    return [result[direction] for direction in ORDER]


def quarters(name):
    """Bits 1 upper left, 2 upper right, 4 lower left, 8 lower right, or None."""
    if not name.startswith("QUADRANT "):
        return None
    return sum(QUARTERS[part] for part in name.removeprefix("QUADRANT ").split(" AND "))


def tables():
    lines = [BEGIN, "/// Arms of U+2500 to U+257F: up, right, down, left; 0 none, 1 light, 2 heavy, 3 double.\n"]
    lines.append("/// All zero for dashes, arcs and diagonals, which stay with the font.\n")
    lines.append("#[rustfmt::skip]\nconst ARMS: [[u8; 4]; 0x80] = [\n")
    for code in range(0x2500, 0x2580):
        name = unicodedata.name(chr(code))
        values = arms(name) or [0, 0, 0, 0]
        lines.append(f"    [{', '.join(map(str, values))}], // {chr(code)} {name.removeprefix('BOX DRAWINGS ').lower()}\n")
    lines.append("];\n\n")
    lines.append("/// Quarters of U+2596 to U+259F: 1 upper left, 2 upper right, 4 lower left, 8 lower right.\n")
    lines.append("#[rustfmt::skip]\nconst QUARTERS: [u8; 10] = [\n")
    for code in range(0x2596, 0x25A0):
        name = unicodedata.name(chr(code))
        lines.append(f"    {quarters(name)}, // {chr(code)} {name.lower()}\n")
    lines.append("];\n")
    lines.append(END)
    return "".join(lines)


def main():
    text = TARGET.read_text(encoding="utf-8")
    start, end = text.index(BEGIN), text.index(END) + len(END)
    updated = text[:start] + tables() + text[end:]
    if "--check" in sys.argv:
        if updated != text:
            print("crates/core/src/linedraw.rs: the tables are out of date; run tools/box_drawing.py")
            return 1
        return 0
    TARGET.write_text(updated, encoding="utf-8")
    return 0


if __name__ == "__main__":
    sys.exit(main())
