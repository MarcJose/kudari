#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
"""The data and the report of tools/bench.sh.

    tools/bench.py data DIR                 the text and the colored text
    tools/bench.py frames DIR ROWS COLUMNS  300 full screens of a terminal of that size
    tools/bench.py report DIR [TERMINAL...]  the runs in DIR, in the order of the terminals

The data comes from a fixed seed, so every run gets the same. For Kudari, the report also
counts the frames that Qt Quick drew during each load and how long it took to prepare them
("sync"), which is where a QQuickPaintedItem paints.
"""

import argparse
import datetime
import random
import re
import statistics
from pathlib import Path

WORDS = "lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor".split()
CELLS = "abcdefghijklmnopqrstuvwxyz0123456789#@%&"
LOADS = ("plain", "color", "frames", "seq")
DESCRIPTIONS = {
    "plain": "24 MB of plain text",
    "color": "28 MB of text in 256 colors",
    "frames": "300 full screens with colors of their own in every cell",
    "seq": "the numbers up to 2,000,000",
}
SEQ_BYTES = sum(len(f"{n}\n") for n in range(1, 2_000_001))
FRAME = re.compile(r"^(\S+) .*render thread.*frame rendered in \d+ms, sync=(\d+), render=\d+, swap=\d+")
# A frame shows the output a moment after it arrived: frames this long after a load count.
TAIL = 0.05


def data(directory):
    rng = random.Random(1)
    with open(directory / "plain.txt", "w") as text:
        for number in range(400_000):
            text.write(f"{number:7d} " + " ".join(rng.choice(WORDS) for _ in range(8)) + "\n")
    with open(directory / "color.txt", "w") as text:
        for _ in range(200_000):
            words = (f"\x1b[38;5;{rng.randrange(256)}m{rng.choice(WORDS)}" for _ in range(8))
            text.write(" ".join(words) + "\x1b[0m\n")


def frames(directory, rows, columns):
    """Full screens in which every cell has a foreground and a background of its own."""
    path = directory / f"frames-{rows}x{columns}.txt"
    if path.exists():
        return
    rng = random.Random(1)
    with open(path, "w") as text:
        for _ in range(300):
            text.write("\x1b[H")
            for row in range(rows):
                cells = (
                    f"\x1b[38;5;{rng.randrange(256)};48;5;{rng.randrange(256)}m{rng.choice(CELLS)}"
                    for _ in range(columns)
                )
                text.write("".join(cells) + ("\r\n" if row < rows - 1 else ""))
            text.write("\x1b[0m")


def when(stamp):
    return datetime.datetime.fromisoformat(stamp).timestamp()


def measure(directory, name):
    """Per load of one run: seconds, bytes, the sync times of the frames meanwhile, and the
    seconds in which they were counted."""
    lines = (directory / f"{name}.txt").read_text().splitlines()
    rows, columns = lines[0].split()[1:]
    sizes = {
        "plain": (directory / "plain.txt").stat().st_size,
        "color": (directory / "color.txt").stat().st_size,
        "frames": (directory / f"frames-{rows}x{columns}.txt").stat().st_size,
        "seq": SEQ_BYTES,
    }
    drawn = []
    log = directory / f"{name}.log"
    for line in log.read_text(errors="replace").splitlines():
        match = FRAME.match(line)
        if match:
            drawn.append((when(match[1]), int(match[2])))
    result = {}
    for line in lines[1:]:
        parts = line.split()
        if len(parts) != 3:
            continue
        load, start, end = parts[0], when(parts[1]), when(parts[2])
        syncs = [sync for moment, sync in drawn if start <= moment <= end + TAIL]
        result[load] = (end - start, sizes[load], syncs, end - start + TAIL)
    return f"{columns}x{rows}", result


def milliseconds(whole):
    """A time that Qt cut off to whole milliseconds, as the range it stands for."""
    return "<1" if whole == 0 else f"{whole}-{whole + 1}"


def table(header, rows):
    """Columns as wide as their widest cell, the first one to the left, the others right."""
    widths = [max(len(cell) for cell in column) for column in zip(header, *rows, strict=True)]

    def line(cells):
        padded = (cell.ljust(width) if index == 0 else cell.rjust(width) for index, (cell, width) in enumerate(zip(cells, widths, strict=True)))
        return "  ".join(padded).rstrip()

    print(line(header))
    print("  ".join("-" * width for width in widths))
    for row in rows:
        print(line(row))


def report(directory, order):
    runs = {}
    for path in sorted(directory.glob("*-*.txt")):
        terminal, run = path.stem.rsplit("-", 1)
        # The results of a run, not the screens of the frames load.
        if run.isdigit():
            runs.setdefault(terminal, []).append(measure(directory, path.stem))
    terminals = [t for t in order if t in runs] + sorted(t for t in runs if t not in order)
    count = max(len(measured) for measured in runs.values())
    print(f"\n{count} runs per terminal; seconds per load (average, fastest and slowest run),")
    print("and the throughput of the average. The terminals differ in their fonts and so in their")
    print("cells, and the frames load in its size: the highest throughput comes first.")
    for load in LOADS:
        rows = []
        for terminal in terminals:
            done = [result[load] for _, result in runs[terminal] if load in result]
            if not done:
                continue
            seconds = [took for took, _, _, _ in done]
            average = statistics.mean(seconds)
            size = statistics.mean(size for _, size, _, _ in done)
            cells = runs[terminal][0][0]
            rows.append((-size / average, [terminal, cells, f"{average:.2f}", f"{min(seconds):.2f}", f"{max(seconds):.2f}", f"{size / average / 1e6:.1f}", str(len(done))]))
        if not rows:
            continue
        print(f"\n{load}: {DESCRIPTIONS[load]}")
        table(["terminal", "cells", "avg s", "min s", "max s", "MB/s", "runs"], [row for _, row in sorted(rows, key=lambda r: r[0])])
    # Only Qt Quick reports its frames: those of Kudari.
    framed = {t: m for t, m in runs.items() if any(syncs for _, result in m for _, _, syncs, _ in result.values())}
    for terminal, measured in framed.items():
        rows = []
        for load in LOADS:
            done = [result[load] for _, result in measured if load in result and result[load][2]]
            if not done:
                continue
            # Over the same seconds in which the frames were counted.
            rates = [len(syncs) / window for _, _, syncs, window in done]
            syncs = [sync for _, _, run, _ in done for sync in run]
            # The middle of each millisecond that Qt cut off, which is right on average when
            # the rest is spread evenly.
            middle = statistics.mean(syncs) + 0.5
            rows.append([load, f"{statistics.mean(rates):.1f}", f"{min(rates):.1f}", f"{max(rates):.1f}", f"{middle:.1f}", milliseconds(min(syncs)), milliseconds(max(syncs))])
        print(f"\n{terminal}: frames that Qt Quick drew during each load, and the milliseconds it took to prepare one")
        print("(Qt gives whole milliseconds, cut off: the average is that of their middles)")
        table(["load", "avg fps", "min fps", "max fps", "avg ms", "min ms", "max ms"], rows)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("action", choices=("data", "frames", "report"))
    parser.add_argument("directory", type=Path)
    parser.add_argument("rest", nargs="*", help="rows and columns for frames, terminals for report")
    arguments = parser.parse_args()
    if arguments.action == "data":
        data(arguments.directory)
    elif arguments.action == "frames":
        if len(arguments.rest) != 2 or not all(part.isdigit() for part in arguments.rest):
            parser.error("frames needs the rows and the columns")
        frames(arguments.directory, *map(int, arguments.rest))
    else:
        report(arguments.directory, arguments.rest)


if __name__ == "__main__":
    main()
