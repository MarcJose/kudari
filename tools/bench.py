#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
"""The data and the report of tools/bench.sh.

    tools/bench.py data DIR                 the text and the colored text
    tools/bench.py frames DIR ROWS COLUMNS  300 full screens of a terminal of that size
    tools/bench.py environment DIR [BINARY] what was measured, on what, and how
    tools/bench.py order RUN TERMINAL...    the terminals in the order of a run, with --seed
    tools/bench.py report DIR [TERMINAL...]  the runs in DIR, in the order of the terminals;
                                            --detail adds the times of the frames in detail

The data comes from a fixed seed, so every run gets the same. For Kudari, the report also
counts the frames that Qt Quick drew during each load and how long it took to prepare them
("sync"), which is where a QQuickPaintedItem paints.
"""

import argparse
import datetime
import hashlib
import os
import platform
import random
import re
import statistics
import subprocess
from pathlib import Path
from typing import NamedTuple

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
# The line of a frame in the log of Qt Quick, from each of its render loops: threaded, basic
# and software. Qt gives whole milliseconds there, cut off.
FRAME = re.compile(
    r"^(\S+) (?:.*\[(render|gui) thread[^\]]*\].*[Ff]rame rendered|.*Frame rendered with '(\w+)' renderloop) "
    r"in (\d+)ms, (?:polish=\d+, )?sync=(\d+), render=(\d+), swap=(\d+)"
)
# What the GUI thread did for a frame of the threaded render loop, also in whole milliseconds.
PREPARED = re.compile(r"^(\S+) .*\[gui thread\] Frame prepared, .*blockedForSync=(\d+) ms")
# A frame shows the output a moment after it arrived: frames this long after a load count.
TAIL = 0.05
# A gap between two frames this long, in milliseconds, left out one at 60 Hz.
LATE = 1.5 * 1000 / 60
# The time a frame has at a refresh rate, in milliseconds.
BUDGETS = (("60 Hz", 1000 / 60), ("120 Hz", 1000 / 120), ("144 Hz", 1000 / 144))
# The seed of the bootstrap intervals, so that a report comes out the same again.
SEED = 1
# How far from the median a run must be at least to count as an outlier, as a share of it.
OUTLIER = 0.03


class Frame(NamedTuple):
    time: float
    total: int
    sync: int
    render: int
    swap: int
    loop: str


class Load(NamedTuple):
    seconds: float
    size: int
    frames: list
    blocked: list
    window: float


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


def frame(line):
    """A frame in the log: its time, the milliseconds of the whole frame and of sync, render
    and swap, and the render loop; or None."""
    match = FRAME.match(line)
    if not match:
        return None
    loop = match[3] or ("threaded" if match[2] == "render" else "basic")
    return Frame(when(match[1]), *(int(match[group]) for group in range(4, 8)), loop)


def prepared(line):
    """How long the GUI thread waited for the render thread in a frame of the threaded loop:
    its time and blockedForSync in milliseconds; or None."""
    match = PREPARED.match(line)
    return (when(match[1]), int(match[2])) if match else None


def measure(directory, name):
    """Per load of one run: its seconds, its bytes, the frames and the waits of the GUI thread
    meanwhile, and the seconds in which they were counted; with the render loops seen."""
    lines = (directory / f"{name}.txt").read_text().splitlines()
    rows, columns = lines[0].split()[1:]
    sizes = {
        "plain": (directory / "plain.txt").stat().st_size,
        "color": (directory / "color.txt").stat().st_size,
        "frames": (directory / f"frames-{rows}x{columns}.txt").stat().st_size,
        "seq": SEQ_BYTES,
    }
    drawn, waits, loops = [], [], set()
    log = directory / f"{name}.log"
    for line in log.read_text(errors="replace").splitlines():
        if found := frame(line):
            drawn.append(found)
            loops.add(found.loop)
        elif found := prepared(line):
            waits.append(found)
    result = {}
    for line in lines[1:]:
        parts = line.split()
        if len(parts) != 3:
            continue
        load, start, end = parts[0], when(parts[1]), when(parts[2])
        frames_meanwhile = [found for found in drawn if start <= found.time <= end + TAIL]
        blocked = [wait for moment, wait in waits if start <= moment <= end + TAIL]
        result[load] = Load(end - start, sizes[load], frames_meanwhile, blocked, end - start + TAIL)
    return f"{columns}x{rows}", result, loops


def milliseconds(whole):
    """A time that Qt cut off to whole milliseconds, as the range it stands for."""
    return "<1" if whole == 0 else f"{whole}-{whole + 1}"


def percentile(values, share):
    """The value below which `share` of the values lie, as the nearest of them."""
    ordered = sorted(values)
    return ordered[min(len(ordered) - 1, max(0, round(share * len(ordered) + 0.5) - 1))]


def spread(values):
    """The median and the median absolute deviation."""
    middle = statistics.median(values)
    return middle, statistics.median(abs(value - middle) for value in values)


def interval(values, seed=SEED, rounds=2000):
    """A 95 % bootstrap interval of the median: medians of samples drawn with replacement."""
    rng = random.Random(seed)
    medians = sorted(statistics.median(rng.choices(values, k=len(values))) for _ in range(rounds))
    return medians[int(0.025 * rounds)], medians[int(0.975 * rounds) - 1]


def outliers(numbered):
    """The runs further than three median absolute deviations from the median, and at least
    OUTLIER of it: with a few runs close together, the deviation is so small that the clock
    alone would make outliers."""
    middle, deviation = spread([value for _, value in numbered])
    return [run for run, value in numbered if abs(value - middle) > max(3 * deviation, OUTLIER * middle)]


def order(seed, run, terminals):
    """The terminals in the order of one run: shuffled, the same again for the same seed."""
    shuffled = list(terminals)
    random.Random(f"{seed}-{run}").shuffle(shuffled)
    return shuffled


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


def output(*command, environment=None):
    """What a command prints, or None when it is missing or fails."""
    try:
        done = subprocess.run(command, capture_output=True, text=True, timeout=20, check=True, env=environment)
    except (OSError, subprocess.SubprocessError):
        return None
    return done.stdout.strip()


def content(path):
    try:
        return Path(path).read_text().strip()
    except OSError:
        return None


def digest(path):
    hashed = hashlib.sha256()
    with open(path, "rb") as file:
        for block in iter(lambda: file.read(1 << 20), b""):
            hashed.update(block)
    return hashed.hexdigest()


def makepkg_flags():
    """The RUSTFLAGS and CXXFLAGS that makepkg builds with, from its configuration."""
    script = (
        'for file in /etc/makepkg.conf /etc/makepkg.conf.d/*.conf "${XDG_CONFIG_HOME:-$HOME/.config}/pacman/makepkg.conf" "$HOME/.makepkg.conf"; do '
        '[ -r "$file" ] && . "$file"; done; printf "%s\\n%s\\n" "$RUSTFLAGS" "$CXXFLAGS"'
    )
    flags = output("bash", "-c", script)
    if flags is None:
        return None
    rust, _, cxx = flags.partition("\n")
    return " ".join(rust.split()), " ".join(cxx.split())


def build(binary, local):
    """What tells the measured Kudari apart from another build: version, origin, hashes,
    compilers and, for a package, the flags of makepkg."""
    path = Path(binary)
    facts = {"path": str(path), "sha256": digest(path), "modified": path.stat().st_mtime}
    # --version builds the application of Qt, which needs no screen this way.
    facts["version"] = output(str(path), "--version", environment={**os.environ, "QT_QPA_PLATFORM": "offscreen"})
    notes = output("readelf", "-n", str(path)) or ""
    found = re.search(r"Build ID: (\w+)", notes)
    facts["build id"] = found[1] if found else None
    comment = output("readelf", "-p", ".comment", str(path)) or ""
    tools = []
    for pattern, name in ((r"rustc version (\S+)", "rustc"), (r"GCC: \([^)]*\) (\S+)", "GCC"), (r"clang version (\S+)", "clang"), (r"LLD (\S+)", "LLD")):
        found = re.search(pattern, comment)
        if found:
            tools.append(f"{name} {found[1]}")
    facts["compilers"] = tools
    sections = output("readelf", "-S", "-W", str(path)) or ""
    facts["debug info"] = ".debug_info" in sections
    owner = output("pacman", "-Qo", str(path))
    found = re.search(r"is owned by (.+)$", owner or "")
    facts["package"] = found[1] if found else None
    if facts["package"]:
        facts["makepkg"] = makepkg_flags()
    local = Path(local) if local else None
    if local and local.exists() and local.resolve() != path.resolve():
        facts["newer local build"] = str(local) if local.stat().st_mtime > facts["modified"] else None
    if local and local.exists() and local.resolve() == path.resolve():
        checkout = Path(__file__).resolve().parent.parent
        facts["git"] = output("git", "-C", str(checkout), "describe", "--tags", "--always", "--dirty")
        facts["rustflags"] = os.environ.get("RUSTFLAGS") or os.environ.get("CARGO_ENCODED_RUSTFLAGS", "").replace("\x1f", " ") or None
    return facts


def computer():
    """The processor, its clock and the load, which change how fast short loads run."""
    facts = {"kernel": platform.release(), "threads": os.cpu_count(), "load": os.getloadavg()[0]}
    found = re.search(r"^model name\s*:\s*(.+)$", content("/proc/cpuinfo") or "", re.M)
    facts["processor"] = found[1] if found else platform.machine()
    frequency = Path("/sys/devices/system/cpu/cpu0/cpufreq")
    facts["driver"] = content(frequency / "scaling_driver")
    facts["governor"] = content(frequency / "scaling_governor")
    facts["epp"] = content(frequency / "energy_performance_preference")
    boost = content("/sys/devices/system/cpu/cpufreq/boost") or content(frequency / "boost")
    if boost is None and (no_turbo := content("/sys/devices/system/cpu/intel_pstate/no_turbo")) is not None:
        boost = "0" if no_turbo == "1" else "1"
    facts["boost"] = None if boost is None else boost == "1"
    facts["power profile"] = output("powerprofilesctl", "get")
    mains = [content(supply / "online") for supply in Path("/sys/class/power_supply").glob("*") if content(supply / "type") == "Mains"]
    facts["on battery"] = bool(mains) and all(online == "0" for online in mains)
    return facts


def versions():
    found = {}
    found["Qt"] = output("qmake6", "-query", "QT_VERSION") or output("qtpaths6", "--qt-version")
    kwin = output("kwin_wayland", "--version")
    found["KWin"] = kwin.split()[-1] if kwin else None
    mesa = output("pacman", "-Q", "mesa")
    found["Mesa"] = mesa.split()[-1].split(":")[-1].rsplit("-", 1)[0] if mesa else None
    return found


class Plan(NamedTuple):
    runs: int
    warmup: int
    seed: str


def describe(kudari, machine, software, removed, kept, plan=None):
    """The lines of the head of the report, and the warnings about the measurement."""
    lines, warnings = [], []
    if plan:
        warm = (f" after {plan.warmup} warm-up runs that do not count" if plan.warmup != 1 else " after 1 warm-up run that does not count") if plan.warmup else ""
        lines.append(f"Runs      {plan.runs}{warm}; the terminals in an order shuffled for each run, --seed {plan.seed} repeats it")
    if kudari:
        parts = [kudari["version"] or "no --version"]
        if kudari.get("git"):
            parts.append(f"git {kudari['git']}")
        if kudari["package"]:
            parts.append(f"package {kudari['package']}")
        parts.append(f"sha256 {kudari['sha256'][:16]}")
        if kudari["build id"]:
            parts.append(f"build ID {kudari['build id'][:16]}")
        parts.extend(kudari["compilers"])
        parts.append("with debug info" if kudari["debug info"] else "without debug info")
        lines.append(f"Kudari    {kudari['path']}: {', '.join(parts)}")
        if kudari.get("makepkg"):
            rust, cxx = kudari["makepkg"]
            lines.append(f"          makepkg builds with RUSTFLAGS={rust or '(none)'} CXXFLAGS={cxx or '(none)'}")
        if kudari.get("rustflags"):
            lines.append(f"          RUSTFLAGS here: {kudari['rustflags']}, if the build had them too")
        if kudari.get("newer local build"):
            warnings.append(f"{kudari['newer local build']} is newer than the Kudari measured; name it with --binary to measure it")
    clock = [machine["driver"] or "no cpufreq"]
    if machine["governor"]:
        clock.append(f"governor {machine['governor']}")
    if machine["epp"]:
        clock.append(f"EPP {machine['epp']}")
    if machine["boost"] is not None:
        clock.append("boost on" if machine["boost"] else "boost off")
    if machine["power profile"]:
        clock.append(f"power profile {machine['power profile']}")
    lines.append(f"Computer  {machine['processor']}, {machine['threads']} threads; Linux {machine['kernel']}; {', '.join(clock)}; load {machine['load']:.2f}")
    lines.append("Versions  " + ", ".join(f"{name} {version or 'unknown'}" for name, version in software.items()))
    if removed:
        verb = "kept, with --keep-env" if kept else "removed from the environment (--keep-env keeps them)"
        lines.append(f"Variables {', '.join(removed)}: {verb}")
    if machine["load"] > 1:
        warnings.append(f"the computer was busy (load {machine['load']:.2f}); other programs take time from the terminals")
    if machine["on battery"]:
        warnings.append("the computer runs on battery, which may slow it down")
    if "performance" not in (machine["governor"], machine["epp"]) and machine["driver"]:
        warnings.append(
            "neither the governor nor the EPP is performance, so short loads depend on how fast the processor speeds up; "
            "for steadier numbers: powerprofilesctl launch -p performance -- tools/bench.sh ..."
        )
    return lines + [f"Warning: {warning}" for warning in warnings]


def environment(directory, binary, local, removed, kept, plan=None):
    """Writes the head of the report to DIR/environment.txt and shows it."""
    kudari = build(binary, local) if binary else None
    text = "\n".join(describe(kudari, computer(), versions(), removed, kept, plan)) + "\n"
    (directory / "environment.txt").write_text(text)
    print(text, end="")


def gaps(frames):
    """The milliseconds from each frame to the next."""
    times = [found.time for found in frames]
    return [(later - earlier) * 1000 for earlier, later in zip(times, times[1:])]


def throughput(runs, terminals):
    """Per load, the seconds of each terminal over its runs."""
    for load in LOADS:
        rows = []
        for terminal in terminals:
            done = [(number, result[load]) for number, (_, result, _) in runs[terminal] if load in result]
            if not done:
                continue
            seconds = [measured.seconds for _, measured in done]
            middle, deviation = spread(seconds)
            low, high = interval(seconds)
            size = statistics.mean(measured.size for _, measured in done)
            late = outliers([(number, measured.seconds) for number, measured in done])
            cells = runs[terminal][0][1][0]
            rows.append(
                (
                    -size / middle,
                    [
                        terminal,
                        cells,
                        f"{middle:.2f}",
                        f"{100 * deviation / middle:.1f}",
                        f"{low:.2f}-{high:.2f}",
                        f"{min(seconds):.2f}",
                        f"{max(seconds):.2f}",
                        f"{size / middle / 1e6:.1f}",
                        str(len(done)),
                        ", ".join(late) or "-",
                    ],
                )
            )
        if not rows:
            continue
        print(f"\n{load}: {DESCRIPTIONS[load]}")
        table(
            ["terminal", "cells", "median s", "MAD %", "95% median s", "min s", "max s", "MB/s", "runs", "outliers"],
            [row for _, row in sorted(rows, key=lambda row: row[0])],
        )


def drawing(terminal, measured, detail):
    """For a terminal that reports its frames: how often and how fast it drew during each load."""
    rows, details = [], []
    for load in LOADS:
        done = [result[load] for _, (_, result, _) in measured if load in result and result[load].frames]
        if not done:
            continue
        rates = [len(load_run.frames) / load_run.window for load_run in done]
        frames = [found for load_run in done for found in load_run.frames]
        between = [gap for load_run in done for gap in gaps(load_run.frames)]
        blocked = [wait for load_run in done for wait in load_run.blocked]
        syncs = [found.sync for found in frames]
        late = f"{100 * sum(gap > LATE for gap in between) / len(between):.0f}" if between else "-"
        share = f"{100 * sum(blocked) / 1000 / sum(load_run.window for load_run in done):.0f}" if blocked else "-"
        rows.append([load, f"{statistics.median(rates):.1f}", late, milliseconds(percentile(syncs, 0.5)), milliseconds(percentile(syncs, 0.95)), milliseconds(max(syncs)), share])
        details.append((load, frames, between, blocked))
    loops = sorted({loop for _, (_, _, seen) in measured for loop in seen})
    print(f"\n{terminal}: frames that Qt Quick drew during each load (render loop {', '.join(loops)})")
    print(f"fps is the median over the runs; late are the gaps between frames of more than {LATE:.0f} ms, one at")
    print("60 Hz left out; sync is where Kudari paints, in whole milliseconds as Qt gives them, cut off;")
    print("GUI blocked is the share of the time the GUI thread waited for the render thread, at least.")
    table(["load", "fps", "late %", "sync p50", "sync p95", "sync max", "GUI blocked %"], rows)
    if not detail:
        return
    print("\nIn detail, over all runs. Qt cuts off to whole milliseconds; swap includes the wait for the")
    print("next VSync, so it shows the upload of the image only together with that wait.")
    for load, frames, between, blocked in details:
        print(f"\n{terminal}, {load}: {len(frames)} frames")
        quantities = [
            ("sync", [found.sync for found in frames], 1),
            ("render", [found.render for found in frames], 1),
            ("swap", [found.swap for found in frames], 1),
            ("sync+render", [found.sync + found.render for found in frames], 2),
            ("whole frame", [found.total for found in frames], 1),
            ("GUI blocked", blocked, 1),
        ]
        rows = [[name] + [ranged(percentile(values, share), width) for share in (0.5, 0.95, 0.99)] + [ranged(max(values), width)] for name, values, width in quantities if values]
        if between:
            rows.append(["gap"] + [f"{percentile(between, share):.0f}" for share in (0.5, 0.95, 0.99)] + [f"{max(between):.0f}"])
        table(["ms", "p50", "p95", "p99", "max"], rows)
        busy = [found.sync + found.render for found in frames]
        over = " / ".join(f"{100 * sum(value >= budget for value in busy) / len(busy):.0f} %" for _, budget in BUDGETS)
        print(f"sync+render over the budget of {' / '.join(name for name, _ in BUDGETS)}: at least {over}")


def ranged(whole, width):
    """Whole milliseconds cut off, `width` of them added up, as the range they stand for."""
    return milliseconds(whole) if width == 1 else f"{whole}-{whole + width}"


def report(directory, order, detail=False):
    head = directory / "environment.txt"
    if head.exists():
        print()
        print(head.read_text(), end="")
    runs, warmups = {}, 0
    for path in sorted(directory.glob("*-*.txt")):
        terminal, run = path.stem.rsplit("-", 1)
        # The results of a run, not the screens of the frames load; warm-up runs do not count.
        if run.isdigit():
            runs.setdefault(terminal, []).append((run, measure(directory, path.stem)))
        elif re.fullmatch(r"w\d+", run):
            warmups = max(warmups, int(run[1:]))
    if not runs:
        print("\nNo run finished; the logs of the terminals tell why.")
        return
    terminals = [t for t in order if t in runs] + sorted(t for t in runs if t not in order)
    count = max(len(measured) for measured in runs.values())
    after = (f" after {warmups} warm-up runs that do not count" if warmups != 1 else " after 1 warm-up run that does not count") if warmups else ""
    print(f"\n{count} runs per terminal{after}.")
    print("Seconds per load: the median, its median absolute deviation in percent of it, a 95 %")
    print("bootstrap interval of the median (seed 1), the fastest and the slowest run, and the runs")
    print("further than three deviations and 3 % from the median. The throughput is that of the median.")
    print("The terminals differ in their fonts and so in their cells, and the frames load in its size:")
    print("the highest throughput comes first.")
    throughput(runs, terminals)
    # Only Qt Quick reports its frames: those of Kudari.
    for terminal in terminals:
        if terminal == "kudari" and not any(measured.frames for _, (_, result, _) in runs[terminal] for measured in result.values()):
            print("\nkudari: no frame timings in its logs; did Qt honour QSG_RENDER_TIMING, and in which render loop?")
    for terminal in terminals:
        if any(measured.frames for _, (_, result, _) in runs[terminal] for measured in result.values()):
            drawing(terminal, runs[terminal], detail)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("action", choices=("data", "frames", "environment", "order", "report"))
    parser.add_argument("directory", type=Path)
    parser.add_argument("rest", nargs="*", help="rows and columns for frames, the Kudari measured for environment, terminals for report")
    parser.add_argument("--local", help="for environment: the release build of the checkout, to warn when it is newer")
    parser.add_argument("--variables", default="", help="for environment: the variables of Qt, Mesa and the like that were set, with commas")
    parser.add_argument("--keep-env", action="store_true", help="for environment: those variables were kept")
    parser.add_argument("--runs", type=int, default=0, help="for environment: the runs that count")
    parser.add_argument("--warmup", type=int, default=0, help="for environment: the warm-up runs before them")
    parser.add_argument("--seed", default="", help="for environment and order: the seed of the order of the terminals")
    parser.add_argument("--detail", action="store_true", help="for report: the times of the frames in detail")
    arguments = parser.parse_args()
    if arguments.action == "environment":
        if len(arguments.rest) > 1:
            parser.error("environment takes at most the Kudari measured")
        removed = [name for name in arguments.variables.split(",") if name]
        plan = Plan(arguments.runs, arguments.warmup, arguments.seed) if arguments.runs else None
        environment(arguments.directory, arguments.rest[0] if arguments.rest else None, arguments.local, removed, arguments.keep_env, plan)
    elif arguments.action == "order":
        # The directory stands for the run here, to keep the arguments in one place.
        print(" ".join(order(arguments.seed, str(arguments.directory), arguments.rest)))
    elif arguments.action == "data":
        data(arguments.directory)
    elif arguments.action == "frames":
        if len(arguments.rest) != 2 or not all(part.isdigit() for part in arguments.rest):
            parser.error("frames needs the rows and the columns")
        frames(arguments.directory, *map(int, arguments.rest))
    else:
        report(arguments.directory, arguments.rest, arguments.detail)


if __name__ == "__main__":
    main()
