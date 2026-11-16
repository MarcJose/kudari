#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
"""The data and the report of tools/bench.sh.

    tools/bench.py data DIR                 the text and the colored text
    tools/bench.py frames DIR ROWS COLUMNS  300 full screens of a terminal of that size
    tools/bench.py environment DIR [BINARY] what was measured, on what, and how
    tools/bench.py report DIR [TERMINAL...]  the runs in DIR, in the order of the terminals

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
    r"in \d+ms, (?:polish=\d+, )?sync=(\d+), render=\d+, swap=\d+"
)
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


def frame(line):
    """The time, the sync in whole milliseconds and the render loop of a frame in the log."""
    match = FRAME.match(line)
    if not match:
        return None
    loop = match[3] or ("threaded" if match[2] == "render" else "basic")
    return when(match[1]), int(match[4]), loop


def measure(directory, name):
    """Per load of one run: seconds, bytes, the sync times of the frames meanwhile, and the
    seconds in which they were counted; with the render loops seen."""
    lines = (directory / f"{name}.txt").read_text().splitlines()
    rows, columns = lines[0].split()[1:]
    sizes = {
        "plain": (directory / "plain.txt").stat().st_size,
        "color": (directory / "color.txt").stat().st_size,
        "frames": (directory / f"frames-{rows}x{columns}.txt").stat().st_size,
        "seq": SEQ_BYTES,
    }
    drawn = []
    loops = set()
    log = directory / f"{name}.log"
    for line in log.read_text(errors="replace").splitlines():
        found = frame(line)
        if found:
            drawn.append(found[:2])
            loops.add(found[2])
    result = {}
    for line in lines[1:]:
        parts = line.split()
        if len(parts) != 3:
            continue
        load, start, end = parts[0], when(parts[1]), when(parts[2])
        syncs = [sync for moment, sync in drawn if start <= moment <= end + TAIL]
        result[load] = (end - start, sizes[load], syncs, end - start + TAIL)
    return f"{columns}x{rows}", result, loops


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


def describe(kudari, machine, software, removed, kept):
    """The lines of the head of the report, and the warnings about the measurement."""
    lines, warnings = [], []
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


def environment(directory, binary, local, removed, kept):
    """Writes the head of the report to DIR/environment.txt and shows it."""
    kudari = build(binary, local) if binary else None
    text = "\n".join(describe(kudari, computer(), versions(), removed, kept)) + "\n"
    (directory / "environment.txt").write_text(text)
    print(text, end="")


def report(directory, order):
    head = directory / "environment.txt"
    if head.exists():
        print()
        print(head.read_text(), end="")
    runs = {}
    for path in sorted(directory.glob("*-*.txt")):
        terminal, run = path.stem.rsplit("-", 1)
        # The results of a run, not the screens of the frames load.
        if run.isdigit():
            runs.setdefault(terminal, []).append(measure(directory, path.stem))
    if not runs:
        print("\nNo run finished; the logs of the terminals tell why.")
        return
    terminals = [t for t in order if t in runs] + sorted(t for t in runs if t not in order)
    count = max(len(measured) for measured in runs.values())
    print(f"\n{count} runs per terminal; seconds per load (average, fastest and slowest run),")
    print("and the throughput of the average. The terminals differ in their fonts and so in their")
    print("cells, and the frames load in its size: the highest throughput comes first.")
    for load in LOADS:
        rows = []
        for terminal in terminals:
            done = [result[load] for _, result, _ in runs[terminal] if load in result]
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
    for terminal in terminals:
        if terminal == "kudari" and not any(syncs for _, result, _ in runs[terminal] for _, _, syncs, _ in result.values()):
            print("\nkudari: no frame timings in its logs; did Qt honour QSG_RENDER_TIMING, and in which render loop?")
    framed = {t: m for t, m in runs.items() if any(syncs for _, result, _ in m for _, _, syncs, _ in result.values())}
    for terminal, measured in framed.items():
        rows = []
        for load in LOADS:
            done = [result[load] for _, result, _ in measured if load in result and result[load][2]]
            if not done:
                continue
            # Over the same seconds in which the frames were counted.
            rates = [len(syncs) / window for _, _, syncs, window in done]
            syncs = [sync for _, _, run, _ in done for sync in run]
            # The middle of each millisecond that Qt cut off, which is right on average when
            # the rest is spread evenly.
            middle = statistics.mean(syncs) + 0.5
            rows.append([load, f"{statistics.mean(rates):.1f}", f"{min(rates):.1f}", f"{max(rates):.1f}", f"{middle:.1f}", milliseconds(min(syncs)), milliseconds(max(syncs))])
        loops = sorted({loop for _, _, seen in measured for loop in seen})
        print(f"\n{terminal}: frames that Qt Quick drew during each load, and the milliseconds it took to prepare one")
        print(f"(render loop {', '.join(loops)}; Qt gives whole milliseconds, cut off: the average is that of their middles)")
        table(["load", "avg fps", "min fps", "max fps", "avg ms", "min ms", "max ms"], rows)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("action", choices=("data", "frames", "environment", "report"))
    parser.add_argument("directory", type=Path)
    parser.add_argument("rest", nargs="*", help="rows and columns for frames, the Kudari measured for environment, terminals for report")
    parser.add_argument("--local", help="for environment: the release build of the checkout, to warn when it is newer")
    parser.add_argument("--variables", default="", help="for environment: the variables of Qt, Mesa and the like that were set, with commas")
    parser.add_argument("--keep-env", action="store_true", help="for environment: those variables were kept")
    arguments = parser.parse_args()
    if arguments.action == "environment":
        if len(arguments.rest) > 1:
            parser.error("environment takes at most the Kudari measured")
        removed = [name for name in arguments.variables.split(",") if name]
        environment(arguments.directory, arguments.rest[0] if arguments.rest else None, arguments.local, removed, arguments.keep_env)
    elif arguments.action == "data":
        data(arguments.directory)
    elif arguments.action == "frames":
        if len(arguments.rest) != 2 or not all(part.isdigit() for part in arguments.rest):
            parser.error("frames needs the rows and the columns")
        frames(arguments.directory, *map(int, arguments.rest))
    else:
        report(arguments.directory, arguments.rest)


if __name__ == "__main__":
    main()
