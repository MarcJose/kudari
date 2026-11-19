#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
"""The data and the report of tools/bench.sh.

    tools/bench.py data DIR                 the text and the colored text
    tools/bench.py frames DIR ROWS COLUMNS  300 full screens of a terminal of that size, and as
                                            many of panels with lines, blocks and braille
    tools/bench.py environment DIR [BINARY] what was measured, on what, and how
    tools/bench.py order RUN TERMINAL...    the terminals in the order of a run, with --seed
    tools/bench.py cpu FILE PID LOAD WHEN   adds the CPU time of each thread of PID to FILE
    tools/bench.py workload DIR             as the shell of a terminal under test: runs the loads
                                            of KUDARI_BENCH_LOADS and times each of them
    tools/bench.py report DIR [TERMINAL...]  the runs in DIR, in the order of the terminals;
                                            --detail adds the times of the frames in detail
    tools/bench.py scenarios DIR            the runs of tools/bench.sh --scenarios in DIR

The data comes from a fixed seed, so every run gets the same. The loads are timed on the
monotonic clock, from their first byte to the answer of the terminal to a query of the cursor
(DSR) after their last: only then has the terminal read and parsed all of a load, which a
terminal with a large buffer would otherwise seem to be quicker at. For Kudari, the report also
counts the frames that Qt Quick drew during each load and how long it took to prepare them
("sync"), which is where a QQuickPaintedItem paints.
"""

import argparse
import datetime
import hashlib
import json
import math
import os
import platform
import random
import re
import select
import statistics
import subprocess
import sys
import termios
import time
from pathlib import Path
from typing import NamedTuple

WORDS = "lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor".split()
CELLS = "abcdefghijklmnopqrstuvwxyz0123456789#@%&"
LOADS = (
    "plain", "color", "unicode", "frames", "boxes", "seq", "search", "titles", "osc", "bells", "queries", "combining",
    "typing", "burst", "scroll", "resize", "query", "partial", "idle",
)
DESCRIPTIONS = {
    "plain": "24 MB of plain text",
    "color": "28 MB of text in 256 colors",
    "unicode": "20 MB of wide characters, emoji and combining marks in 256 colors",
    "frames": "300 full screens with colors of their own in every cell",
    "boxes": "300 full screens of panels with lines, blocks and braille, as system monitors draw them",
    "seq": "the numbers up to 2,000,000",
    "search": "the plain text with all matches of a search for a word in it highlighted, in Kudari alone",
    "titles": "300,000 changes of the title",
    "osc": "100,000 lines with a working directory, links, marks of the prompt and copies to the clipboard",
    "bells": "300,000 bells",
    "queries": "100,000 queries of the device attributes, in batches of 1,000 each answered before the next",
    "combining": "20,000 lines in which every cell carries four combining marks",
    "typing": "100 keys 50 ms apart, each echoed at once",
    "burst": "100 keys at once, each echoed at once",
    "scroll": "100 turns of the wheel up through a history of 20,000 lines and 100 down, 16 ms apart",
    "resize": "10 steps of the font larger and 10 smaller with a history of 20,000 lines, 0.5 s apart",
    "query": "200 queries each of the cursor, the device attributes and the background, alone and under output",
    "partial": "a line of status changed every 16 ms for 10 seconds, the rest of the screen kept",
    "idle": "10 seconds without output",
}
# Loads that take a fixed time instead of a number of bytes, and how often they change the
# screen meanwhile.
FIXED_TIME = {"typing": 100, "burst": 100, "scroll": 200, "resize": 20, "query": 0, "partial": 625, "idle": 0}
# Text of the load `unicode`: words of wide characters, emoji alone, joined and with a skin
# tone, and letters with combining marks.
WIDE = "日本語 中文字 한국어 テキスト 漢字仮名 終端模擬".split()
EMOJI = ["🙂", "👍🏽", "🚀", "👩‍💻", "🏳️‍🌈", "❤️", "🇩🇪", "🧑🏿‍🔬"]
MARKED = ["e\u0301", "a\u0308", "o\u0302", "n\u0303", "c\u0327", "u\u0308\u0301"]
# The pieces of the load boxes: lines thin, heavy and double; blocks; and braille.
LINES = ("─│┌┐└┘├┤┬┴┼", "━┃┏┓┗┛┣┫┳┻╋", "═║╔╗╚╝╠╣╦╩╬")
BLOCKS = "▏▎▍▌▋▊▉█▀▄▐░▒▓▖▗▘▝"
# The queries of the load queries, how many go out before their answers are read, and the
# answer to one: ESC [ ? attributes c. A terminal may give no answer to more questions than
# it keeps waiting, as Kudari beyond 4096, as from a slow terminal.
QUERIES = 100_000
BATCH = 1000
ANSWERED = re.compile(rb"\x1b\[\?[0-9;]*c")
# The shortcuts of the load resize: a larger font and a smaller one, with Ctrl and = or -, and
# with Shift as well in kitty; its steps each way, and the seconds between two.
ZOOM = {"kitty": ((29, 42, 13), (29, 42, 12))}
ZOOM_KEYS = ((29, 13), (29, 12))
ZOOM_STEPS = 10
ZOOM_GAP = 0.5
# The queries of the load query, each with the pattern of its answer: the place of the cursor
# (DSR), the device attributes (DA1), and the color of the background (OSC 11), which some
# terminals do not answer; a query of the device attributes after it tells that apart.
ASKING = (
    ("cursor", b"\x1b[6n", rb"\x1b\[\d+;\d+R"),
    ("attributes", b"\x1b[c", rb"\x1b\[\?[0-9;]*c"),
    ("background", b"\x1b]11;?\x07", rb"\x1b\]11;[^\x07\x1b]*(?:\x07|\x1b\\)"),
)
ASKED = 200
# The time between two changes of the load partial, in seconds.
PARTIAL_EVERY = 0.016
# The key that the loads typing and burst press, a, as Linux numbers it; how often, and how
# many milliseconds apart in typing.
KEY_A = 30
TYPED = 100
TYPING_GAP = 50
# The turns of the wheel of the load scroll each way, their milliseconds apart and the value
# of one, and the lines of history it scrolls through.
TURNS = 100
TURN_GAP = 16
TURN = 15
HISTORY = 20_000
# The keys that open the search of Kudari (Ctrl+Shift+F), type a word of the plain text and
# close it again (Escape).
OPEN_SEARCH = (29, 42, 33)
SEARCHED = (38, 24, 19, 18, 50)
ESCAPE = 1
# The size of the virtual screen in pixels of the device.
SCREEN = (2560, 1440)
SEQ_BYTES = sum(len(f"{n}\n") for n in range(1, 2_000_001))
# The line of a frame in the log of Qt Quick, from each of its render loops: threaded, basic
# and software. Qt gives whole milliseconds there, cut off. The time in front is the monotonic
# clock in seconds (`%{time boot}`, which Qt pads with spaces), or in older runs the time of day.
FRAME = re.compile(
    r"^\s*(\S+) (?:.*\[(render|gui) thread[^\]]*\].*[Ff]rame rendered|.*Frame rendered with '(\w+)' renderloop) "
    r"in (\d+)ms, (?:polish=\d+, )?sync=(\d+), render=(\d+), swap=(\d+)"
)
# What the GUI thread did for a frame of the threaded render loop, also in whole milliseconds.
PREPARED = re.compile(r"^\s*(\S+) .*\[gui thread\] Frame prepared, .*blockedForSync=(\d+) ms")
# A line of the counters of a build with the feature stats: the seconds since 1970 and JSON.
STATS = re.compile(r"^kudari-stats (\d+(?:\.\d+)?) (\{.*\})$")
# The counted times of the counters, as they name them and as the report does.
TIMED = (("lock_wait", "lock wait"), ("lock_hold", "lock hold"), ("queue", "queue"), ("frame_build", "build"), ("frame_draw", "draw"))
# The steps of the path of a key that the counters follow, one key at a time, as they name
# them and as the report does.
STEPS = (
    ("key_write", "key to its bytes written"),
    ("key_output", "to the output read"),
    ("key_refresh", "to the refresh"),
    ("key_paint", "to the paint"),
    ("key_total", "key to paint in all"),
)
# A frame shows the output a moment after it arrived: frames this long after a load count.
TAIL = 0.05
# Bytes written at a time, as `cat` does.
BLOCK = 128 * 1024
# How long the driver waits for the answer of a terminal to a query of the cursor, in seconds.
ANSWER = 5.0
# Round trips of that query without output, before the loads.
ROUND_TRIPS = 100
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
    # Milliseconds of CPU time per group of threads during the load, or None.
    cpu: dict | None = None
    # How often the threads of each group went to sleep or were put aside meanwhile.
    switches: dict | None = None
    # Kilobytes of memory after the load: resident, proportional, private and the peak, then on
    # the graphics card and through its address table if the terminal draws there; or None.
    memory: tuple | None = None
    # What the counters of a build with the feature stats counted meanwhile; or None.
    counters: dict | None = None
    # Seconds from the last byte written to the answer of the terminal; None in older runs.
    backlog: float | None = None
    # Whether the terminal answered; if not, the load ended with its last byte.
    answered: bool = True
    # For the loads that type: each key as the time it was pressed and the time it came.
    keys: list | None = None
    # For the load queries: how many answers came.
    replies: int | None = None
    # For the load query: the seconds of each round trip by query and state, None unanswered.
    asked: dict | None = None


def cpu(path, pid, load, moment):
    """Adds the CPU time and the switches of each thread of the process PID to the file, at the
    start or the end of a load. Only /proc of a process of the same user is read."""
    lines = []
    for task in sorted(Path(f"/proc/{pid}/task").glob("*")):
        schedstat = content(task / "schedstat")
        name = content(task / "comm")
        status = content(task / "status") or ""
        if schedstat is None or name is None:
            continue
        # The main thread shows the name of the program; it is the one of the GUI.
        name = "main" if task.name == str(pid) else re.sub(r"[\t\n]", " ", name)
        switches = [re.search(rf"^{kind}_ctxt_switches:\s*(\d+)", status, re.M) for kind in ("voluntary", "nonvoluntary")]
        counts = [found[1] if found else "0" for found in switches]
        lines.append(f"{load}\t{moment}\t{task.name}\t{name}\t{schedstat.split()[0]}\t{counts[0]}\t{counts[1]}\n")
    if moment == "end":
        rollup = content(f"/proc/{pid}/smaps_rollup") or ""
        sizes = {key: int(value) for key, value in re.findall(r"^(\w+):\s+(\d+) kB", rollup, re.M)}
        private = sizes.get("Private_Clean", 0) + sizes.get("Private_Dirty", 0)
        # The most it ever held, which a flood can drive up for a moment.
        peak = re.search(r"^VmHWM:\s+(\d+) kB", status, re.M) if (status := content(f"/proc/{pid}/status")) else None
        held = gpu(pid)
        graphics = f"\t{held[0]}\t{held[1]}" if held else ""
        lines.append(f"{load}\tmemory\t{sizes.get('Rss', 0)}\t{sizes.get('Pss', 0)}\t{private}\t{peak[1] if peak else 0}{graphics}\n")
    with open(path, "a") as file:
        file.writelines(lines)


# The units that the kernel gives the sizes of memory in `fdinfo` in, in kilobytes.
UNITS = {"": 1 / 1024, "KiB": 1, "MiB": 1024, "GiB": 1024 * 1024}


def gpu(pid):
    """Kilobytes of memory that the process PID holds on the graphics card (VRAM) and in memory
    the card reaches through its address table (GTT), from the `fdinfo` of its DRM devices; or None
    without one, as with drawing in software."""
    infos = []
    for descriptor in Path(f"/proc/{pid}/fd").glob("*"):
        try:
            if os.readlink(descriptor).startswith("/dev/dri/"):
                infos.append(content(f"/proc/{pid}/fdinfo/{descriptor.name}") or "")
        except OSError:
            continue
    return on_card(infos)


def on_card(infos):
    """VRAM and GTT in kilobytes from the `fdinfo` texts of DRM devices, or None if none of them
    names its client. A device opened twice is one client and counts once."""
    clients = {}
    for info in infos:
        fields = dict(re.findall(r"^(drm-[\w-]+):\s*(.*)$", info, re.M))
        if "drm-client-id" not in fields:
            continue
        sizes = []
        for region in ("vram", "gtt"):
            # Older kernels give drm-memory-REGION only.
            found = re.fullmatch(r"(\d+)\s*(\w*)", (fields.get(f"drm-total-{region}") or fields.get(f"drm-memory-{region}") or "0").strip())
            sizes.append(round(int(found[1]) * UNITS.get(found[2], 0)) if found else 0)
        clients[(fields.get("drm-pdev"), fields["drm-client-id"])] = sizes
    if not clients:
        return None
    return sum(vram for vram, _ in clients.values()), sum(gtt for _, gtt in clients.values())


def monotonic():
    return time.clock_gettime(time.CLOCK_MONOTONIC)


def write_all(descriptor, path):
    """Writes the file to the descriptor in blocks, as `cat` does."""
    with open(path, "rb") as source:
        while block := source.read(BLOCK):
            view = memoryview(block)
            while view:
                view = view[os.write(descriptor, view) :]


def answer(reader, writer, wait=ANSWER):
    """Asks the terminal for the place of its cursor (DSR) and waits for the answer, ESC [ row ;
    column R: the time it came, or None after `wait` seconds. The input must not be canonical."""
    termios.tcflush(reader, termios.TCIFLUSH)
    os.write(writer, b"\x1b[6n")
    deadline = monotonic() + wait
    got = b""
    while not re.search(rb"\x1b\[\d+;\d+R", got):
        left = deadline - monotonic()
        if left <= 0 or not select.select([reader], [], [], left)[0]:
            return None
        got += os.read(reader, 64)
    return monotonic()


def round_trips(reader, writer, count=ROUND_TRIPS):
    """The seconds of `count` queries of the cursor and their answers, without output; those
    without an answer are left out."""
    taken = []
    for _ in range(count):
        asked = monotonic()
        answered = answer(reader, writer, wait=1.0)
        if answered is not None:
            taken.append(answered - asked)
    return taken


def typed(reader, writer, command, count):
    """Runs `command`, which presses a key `count` times and prints the time of each press, and
    echoes each character that comes meanwhile at once: the keys as the time each was pressed
    and the time it came, in order."""
    process = subprocess.Popen(command, stdout=subprocess.PIPE, text=True)
    came = []
    while len(came) < count:
        if not select.select([reader], [], [], ANSWER)[0]:
            break
        got = os.read(reader, 1024)
        moment = monotonic()
        os.write(writer, got)
        came.extend([moment] * len(got))
    pressed = [float(line) for line in process.communicate()[0].split()]
    return list(zip(pressed, came))


def round_trip(reader, writer, query, pattern, wait=1.0):
    """Asks once and waits for the answer: its seconds, or None. Behind each query goes one of
    the device attributes, which every terminal answers: when that answer comes first, the
    terminal does not answer this query, and the wait ends there."""
    sentinel = rb"\x1b\[\?[0-9;]*c"
    guarded = query != b"\x1b[c"
    termios.tcflush(reader, termios.TCIFLUSH)
    asked = monotonic()
    os.write(writer, query + (b"\x1b[c" if guarded else b""))
    got, answered = b"", None
    deadline = asked + wait
    while True:
        if answered is None and re.search(pattern, got):
            answered = monotonic()
        if not guarded and answered is not None:
            return answered - asked
        if guarded and re.search(sentinel, got):
            return None if answered is None else answered - asked
        left = deadline - monotonic()
        if left <= 0 or not select.select([reader], [], [], left)[0]:
            return None if answered is None else answered - asked
        got += os.read(reader, 256)


def queried(reader, writer, busy=None):
    """The seconds of each round trip of each query of the load query, None for those without
    an answer; while `busy` is given, a thread writes that file to the terminal over and over."""
    import threading

    stop = threading.Event()
    if busy is not None:

        def flood():
            while not stop.is_set():
                write_all(writer, busy)

        writing = threading.Thread(target=flood, daemon=True)
        writing.start()
    times = {name: [round_trip(reader, writer, query, pattern) for _ in range(ASKED)] for name, query, pattern in ASKING}
    stop.set()
    if busy is not None:
        writing.join()
    return times


def chord(tool, codes):
    """Presses the keys of a shortcut through the fake input of KWin: the time the last was
    pressed, or None."""
    done = subprocess.run([str(tool), "chord", *map(str, codes)], capture_output=True, text=True, check=False)
    try:
        return float(done.stdout.split()[0])
    except (IndexError, ValueError):
        return None


def press(tool, *codes):
    """Presses the keys together and lets them go, through the fake input of KWin."""
    subprocess.run([str(tool), "key", *map(str, codes)], check=False)
    time.sleep(0.3)


def workload(directory, environment):
    """As the shell of a terminal under test: waits for the window, then runs and times each
    load, and writes a line for each to KUDARI_BENCH_OUT. Only echo and the canonical input of
    the terminal are switched off, for the answers; the output goes out as it would, so that
    a new line is still a carriage return and a line feed."""
    out, loads = Path(environment["KUDARI_BENCH_OUT"]), environment["KUDARI_BENCH_LOADS"].split()
    name = out.stem.rsplit("-", 1)[0].split("@")[0]
    kudari = name == "kudari"
    tool = directory / "input"
    scale = float(environment.get("KUDARI_BENCH_SCALE") or 1)
    middle = [str(size / scale / 2) for size in SCREEN]
    usage, profiled = environment["KUDARI_BENCH_CPU"], environment.get("KUDARI_BENCH_PERF")
    while not (directory / "go").exists():
        time.sleep(0.2)
    terminal = (directory / "pid").read_text().strip()
    time.sleep(1)
    reader, writer = sys.stdin.fileno(), sys.stdout.fileno()
    columns, rows = os.get_terminal_size(writer)
    frames(directory, rows, columns)
    boxes(directory, rows, columns)
    mode = termios.tcgetattr(reader)
    mode[3] &= ~(termios.ICANON | termios.ECHO)
    mode[6][termios.VMIN], mode[6][termios.VTIME] = 1, 0
    termios.tcsetattr(reader, termios.TCSANOW, mode)

    def note(line):
        with open(out, "a") as file:
            file.write(line + "\n")

    note(f"size {rows} {columns}")
    # The counters of Kudari carry the time of day: both clocks at once, to match them.
    note(f"clock {monotonic():.6f} {time.time():.6f}")
    subprocess.run(["clear"], check=False)
    note("round-trips " + " ".join(f"{taken:.6f}" for taken in round_trips(reader, writer)))
    subprocess.run(["clear"], check=False)
    for load in loads:
        if load == "search" and not kudari:
            continue
        time.sleep(1)
        keys, replies, trips_of = None, None, None
        if load in ("scroll", "resize"):
            # A history to scroll through, or to wrap anew.
            subprocess.run(["seq", "1", str(HISTORY)], check=False)
            answer(reader, writer)
        elif load == "search":
            press(tool, *OPEN_SEARCH)
            for code in SEARCHED:
                press(tool, code)
        if load == "partial":
            # A screen full of text that stays, under the line that changes.
            os.write(writer, "\r\n".join(" ".join(WORDS)[:columns] for _ in range(rows - 1)).encode())
            answer(reader, writer)
        # So that the report can name the load during which a terminal ended.
        note(f"begin {load}")
        # The CPU time of the threads of the terminal, just outside the time of the load.
        cpu(usage, terminal, load, "start")
        start = monotonic()
        if load in ("frames", "boxes"):
            write_all(writer, directory / f"{load}-{rows}x{columns}.txt")
        elif load == "partial":
            partial(writer, columns)
        elif load in ("typing", "burst"):
            gap = TYPING_GAP if load == "typing" else 0
            keys = typed(reader, writer, [str(tool), "type", str(KEY_A), str(TYPED), str(gap)], TYPED)
        elif load == "scroll":
            for value in (-TURN, TURN):
                subprocess.run([str(tool), "scroll", *middle, str(value), str(TURNS), str(TURN_GAP)], check=False)
        elif load == "search":
            write_all(writer, directory / "plain.txt")
        elif load == "queries":
            replies = asked(reader, writer, directory / "queries.txt", QUERIES)
        elif load == "resize":
            larger, smaller = ZOOM.get(name, ZOOM_KEYS)
            keys = []
            for codes in [larger] * ZOOM_STEPS + [smaller] * ZOOM_STEPS:
                pressed = chord(tool, codes)
                time.sleep(ZOOM_GAP)
                if pressed is not None:
                    keys.append((pressed, pressed))
        elif load == "query":
            trips_of = {"alone": queried(reader, writer), "busy": queried(reader, writer, directory / f"frames-{rows}x{columns}.txt")}
        elif load == "seq":
            # A program of its own, which writes as it always does.
            subprocess.run(["seq", "1", "2000000"], check=False)
        elif load == "idle":
            time.sleep(10)
        else:
            write_all(writer, directory / f"{load}.txt")
        written = monotonic()
        answered = answer(reader, writer)
        end = answered if answered is not None else written
        if profiled:
            with open(out.with_suffix(".perf"), "a") as file:
                file.write(f"{load} {start:.6f} {end:.6f}\n")
        cpu(usage, terminal, load, "end")
        time.sleep(1)
        note(f"{load} {start:.6f} {end:.6f} {written:.6f} {'answered' if answered is not None else 'unanswered'}")
        if keys is not None:
            note(f"keys {load} " + " ".join(f"{pressed:.6f}:{came:.6f}" for pressed, came in keys))
        if replies is not None:
            note(f"replies {load} {replies}")
        for state, times in (trips_of or {}).items():
            for name, taken in times.items():
                note(f"asked {name} {state} " + " ".join("-" if time is None else f"{time:.6f}" for time in taken))
        if load == "search":
            press(tool, ESCAPE)
        subprocess.run(["clear"], check=False)
    note("done")
    while True:
        time.sleep(100_000)


def threads(path):
    """Per load, the milliseconds of CPU time and the switches per group of threads between its
    start and its end, and the memory after it. A thread that ended meanwhile is missing; one
    that began counts from nothing."""
    if not path.exists():
        return {}, {}, {}
    taken, memory = {}, {}
    for line in path.read_text().splitlines():
        fields = line.split("\t")
        if fields[1] == "memory":
            memory[fields[0]] = tuple(int(field) for field in fields[2:])
            continue
        load, moment, tid, name, nanoseconds, voluntary, other = fields
        taken.setdefault((load, moment), {})[tid] = (name, int(nanoseconds), int(voluntary) + int(other))
    used, woken = {}, {}
    for (load, moment), after in taken.items():
        if moment != "end":
            continue
        before = taken.get((load, "start"), {})
        groups, switches = {}, {}
        for tid, (name, nanoseconds, count) in after.items():
            # Threads of a pool differ only in a number at the end.
            group = re.sub(r"[\s#:]*\d+$", "", name) or name
            _, earlier, earlier_count = before.get(tid, (name, 0, 0))
            groups[group] = groups.get(group, 0) + (nanoseconds - earlier) / 1e6
            switches[group] = switches.get(group, 0) + count - earlier_count
        used[load], woken[load] = groups, switches
    return used, woken, memory


def data(directory):
    rng = random.Random(1)
    with open(directory / "plain.txt", "w") as text:
        for number in range(400_000):
            text.write(f"{number:7d} " + " ".join(rng.choice(WORDS) for _ in range(8)) + "\n")
    with open(directory / "color.txt", "w") as text:
        for _ in range(200_000):
            words = (f"\x1b[38;5;{rng.randrange(256)}m{rng.choice(WORDS)}" for _ in range(8))
            text.write(" ".join(words) + "\x1b[0m\n")
    floods(directory, rng)
    with open(directory / "unicode.txt", "w") as text:
        for number in range(150_000):
            pieces = (rng.choice((rng.choice(WIDE), rng.choice(EMOJI), rng.choice(MARKED) + rng.choice(WORDS), rng.choice(WORDS))) for _ in range(6))
            words = (f"\x1b[38;5;{rng.randrange(256)}m{piece}" for piece in pieces)
            text.write(f"{number:6d} " + " ".join(words) + "\x1b[0m\n")


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


def floods(directory, rng):
    """The loads that flood a terminal with what it has to act on beside its text: titles, other
    operating system commands, bells, queries, and marks that combine with the letters."""
    with open(directory / "titles.txt", "w") as text:
        for number in range(300_000):
            text.write(f"\x1b]0;title {number} {rng.choice(WORDS)}\x07")
            if number % 100 == 99:
                text.write(f"{number}\n")
    with open(directory / "osc.txt", "w") as text:
        for number in range(100_000):
            word = rng.choice(WORDS)
            text.write(
                f"\x1b]133;A\x07$ \x1b]133;B\x07ls {word}\x1b]133;C\x07\x1b]7;file://host/home/{word}/{number}\x07"
                f"\x1b]8;;https://example.org/{word}/{number}\x07{word}\x1b]8;;\x07 {number}"
                + (f"\x1b]52;c;{word.encode().hex()}\x07" if number % 10 == 0 else "")
                + "\x1b]133;D;0\x07\n"
            )
    with open(directory / "bells.txt", "w") as text:
        text.write("\x07" * 300_000)
    with open(directory / "queries.txt", "w") as text:
        text.write("\x1b[c" * QUERIES)
    marks = [chr(code) for code in range(0x0300, 0x0370)]
    with open(directory / "combining.txt", "w") as text:
        for _ in range(20_000):
            cells = (rng.choice("aeiouxyz") + "".join(rng.choice(marks) for _ in range(4)) for _ in range(60))
            text.write("".join(cells) + "\n")


def asked(reader, writer, path, count, batch=BATCH):
    """Writes the queries of the file in batches and reads the answers to each batch before
    the next, as a program that asks does: how many came. A batch without all its answers
    within the wait ends the load."""
    queries = path.read_bytes()
    size = len(queries) // count
    answers = 0
    for first in range(0, count, batch):
        part = queries[first * size : min(count, first + batch) * size]
        os.write(writer, part)
        expected = answers + len(part) // size
        got = b""
        while answers < expected:
            if not select.select([reader], [], [], ANSWER)[0]:
                return answers
            got += os.read(reader, 65536)
            found = ANSWERED.findall(got)
            answers += len(found)
            got = got[got.rfind(b"c") + 1 :] if found else got[-64:]
    return answers


def boxes(directory, rows, columns):
    """Full screens of four panels, each framed in lines of its own kind, with labels, bars of
    blocks and graphs of braille in colors, as system monitors draw them."""
    path = directory / f"boxes-{rows}x{columns}.txt"
    if path.exists():
        return
    rng = random.Random(1)
    half_rows, half_columns = max(3, rows // 2), max(4, columns // 2)
    with open(path, "w") as text:
        for _ in range(300):
            screen = [[" "] * columns for _ in range(rows)]
            colors = [["\x1b[0m"] * columns for _ in range(rows)]
            for top, left in ((0, 0), (0, half_columns), (half_rows, 0), (half_rows, half_columns)):
                bottom, right = min(rows, top + half_rows) - 1, min(columns, left + half_columns) - 1
                if bottom - top < 2 or right - left < 3:
                    continue
                kind = rng.choice(LINES)
                color = f"\x1b[0;38;5;{rng.randrange(256)}m"
                for column in range(left, right + 1):
                    screen[top][column] = screen[bottom][column] = kind[0]
                for row in range(top, bottom + 1):
                    screen[row][left] = screen[row][right] = kind[1]
                screen[top][left], screen[top][right], screen[bottom][left], screen[bottom][right] = kind[2:6]
                for row in range(top, bottom + 1):
                    for column in range(left, right + 1):
                        if row in (top, bottom) or column in (left, right):
                            colors[row][column] = color
                inner = right - left - 1
                for row in range(top + 1, bottom):
                    kind_of_row = (row - top) % 3
                    if kind_of_row == 1:
                        cells = list((rng.choice(WORDS) + " " + f"{rng.randrange(100)}%").ljust(inner)[:inner])
                    elif kind_of_row == 2:
                        filled = rng.randrange(inner + 1)
                        cells = [BLOCKS[7]] * filled + [rng.choice(BLOCKS)] * min(1, inner - filled) + [" "] * max(0, inner - filled - 1)
                    else:
                        cells = [chr(0x2800 + rng.randrange(256)) for _ in range(inner)]
                    shade = f"\x1b[0;38;5;{rng.randrange(256)};48;5;{rng.randrange(256)}m"
                    for offset, cell in enumerate(cells):
                        screen[row][left + 1 + offset] = cell
                        colors[row][left + 1 + offset] = shade
            text.write("\x1b[H")
            for row in range(rows):
                # A color only where it changes, as a program that draws such a screen sends it.
                line, shown = [], None
                for column in range(columns):
                    if colors[row][column] != shown:
                        shown = colors[row][column]
                        line.append(shown)
                    line.append(screen[row][column])
                text.write("".join(line) + "\x1b[0m" + ("\r\n" if row < rows - 1 else ""))


def partial(writer, columns, seconds=10.0, every=PARTIAL_EVERY):
    """Changes a line of status at the top every `every` seconds for `seconds`, as a clock or a
    bar of progress does, and leaves the rest of the screen: the times it changed it."""
    changes = 0
    start = monotonic()
    while (now := monotonic()) - start < seconds:
        filled = changes % (columns - 20)
        line = f"{now - start:7.3f} s " + "█" * filled + "░" * (columns - 20 - filled)
        os.write(writer, f"\x1b7\x1b[1;1H\x1b[7m{line}\x1b[0m\x1b8".encode())
        changes += 1
        time.sleep(max(0.0, start + changes * every - monotonic()))
    return changes


def when(stamp):
    """Seconds on the monotonic clock, or in older runs the time of day."""
    if "T" in stamp:
        return datetime.datetime.fromisoformat(stamp).timestamp()
    return float(stamp)


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


def snapshot(line):
    """The counters in a line of a build with the feature stats: their time and their values;
    or None."""
    match = STATS.match(line)
    if not match:
        return None
    try:
        return float(match[1]), json.loads(match[2])
    except ValueError:
        return None


def counted(snapshots, start, end):
    """What the counters counted from the last line at `start` or before to the first at `end`
    or after: the seconds between those lines, the counts, the paints of all terminals, and
    for each time its count, its sum in microseconds and its buckets; or None without both."""
    before = [found for found in snapshots if found[0] <= start]
    after = [found for found in snapshots if found[0] >= end]
    if not before or not after:
        return None
    (first, old), (last, new) = before[-1], after[0]
    result = {"seconds": last - first}
    for name in ("wakeups", "reports", "refreshes"):
        result[name] = new.get(name, 0) - old.get(name, 0)
    result["paints"] = sum(new.get("paints", {}).values()) - sum(old.get("paints", {}).values())
    for name, _ in TIMED + STEPS:
        was, now = old.get(name, {}), new.get(name, {})
        earlier = was.get("buckets") or []
        buckets = [later - (earlier[index] if index < len(earlier) else 0) for index, later in enumerate(now.get("buckets", []))]
        result[name] = {"count": now.get("count", 0) - was.get("count", 0), "total_us": now.get("total_us", 0) - was.get("total_us", 0), "buckets": buckets}
    return result


def bucket_quantile(buckets, share):
    """The upper end in microseconds of the bucket of powers of two that holds `share` of the
    times, as the counters of Kudari give it; None without any."""
    count = sum(buckets)
    if not count:
        return None
    needed, seen = max(1, math.ceil(count * share)), 0
    for index, found in enumerate(buckets):
        seen += found
        if seen >= needed:
            return 1 << index
    return None


def measure(directory, name):
    """Per load of one run: its seconds, its bytes, the frames and the waits of the GUI thread
    meanwhile, and the seconds in which they were counted; with the render loops seen."""
    lines = (directory / f"{name}.txt").read_text().splitlines()
    rows, columns = lines[0].split()[1:]
    sizes = {
        "plain": (directory / "plain.txt").stat().st_size,
        "color": (directory / "color.txt").stat().st_size,
        "seq": SEQ_BYTES,
        "search": (directory / "plain.txt").stat().st_size,
        **{load: 0 for load in FIXED_TIME},
    }
    # Files that runs before these loads did not make.
    files = {load: f"{load}.txt" for load in ("unicode", "titles", "osc", "bells", "queries", "combining")}
    files.update({load: f"{load}-{rows}x{columns}.txt" for load in ("frames", "boxes")})
    for load, path in files.items():
        if (directory / path).exists():
            sizes[load] = (directory / path).stat().st_size
    drawn, waits, loops, snapshots = [], [], set(), []
    used, woken, memory = threads(directory / f"{name}.cpu")
    log = directory / f"{name}.log"
    for line in log.read_text(errors="replace").splitlines():
        if found := frame(line):
            drawn.append(found)
            loops.add(found.loop)
        elif found := prepared(line):
            waits.append(found)
        elif found := snapshot(line):
            snapshots.append(found)
    result, typing, answered_queries, asked_times = {}, {}, {}, {}
    for line in lines[1:]:
        parts = line.split()
        if parts[0] == "asked" and len(parts) >= 3:
            asked_times[(parts[1], parts[2])] = [None if part == "-" else float(part) for part in parts[3:]]
            continue
        if parts[0] == "replies" and len(parts) == 3:
            answered_queries[parts[1]] = int(parts[2])
            continue
        if parts[0] == "keys" and len(parts) >= 2:
            typing[parts[1]] = [tuple(float(moment) for moment in key.split(":")) for key in parts[2:]]
            continue
        if parts[0] == "clock" and len(parts) == 3:
            # The counters carry the time of day; the loads, the monotonic clock.
            shift = float(parts[2]) - float(parts[1])
            snapshots = [(moment - shift, values) for moment, values in snapshots]
            continue
        if parts[0] in ("size", "round-trips", "done") or len(parts) not in (3, 5):
            continue
        load, start, end = parts[0], when(parts[1]), when(parts[2])
        backlog, answered = (end - float(parts[3]), parts[4] == "answered") if len(parts) == 5 else (None, True)
        frames_meanwhile = [found for found in drawn if start <= found.time <= end + TAIL]
        blocked = [wait for moment, wait in waits if start <= moment <= end + TAIL]
        result[load] = Load(
            end - start,
            sizes.get(load, 0),
            frames_meanwhile,
            blocked,
            end - start + TAIL,
            used.get(load),
            woken.get(load),
            memory.get(load),
            counted(snapshots, start, end + TAIL),
            backlog,
            answered,
        )
    for load, keys in typing.items():
        if load in result:
            result[load] = result[load]._replace(keys=keys)
    for load, count in answered_queries.items():
        if load in result:
            result[load] = result[load]._replace(replies=count)
    if asked_times and "query" in result:
        result["query"] = result["query"]._replace(asked=asked_times)
    return f"{columns}x{rows}", result, loops


def ended(directory, name):
    """The load during which the terminal of a run ended or hung, or None."""
    lines = (directory / f"{name}.txt").read_text().splitlines()
    if "ended" not in lines:
        return None
    begun = [line.split()[1] for line in lines if line.startswith("begin ")]
    return begun[-1] if begun else "?"


def trips(directory, name):
    """The seconds of the queries of the cursor without output in one run; empty in older runs."""
    for line in (directory / f"{name}.txt").read_text().splitlines():
        if line.startswith("round-trips"):
            return [float(part) for part in line.split()[1:]]
    return []


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
    settings: tuple = ()


def describe(kudari, machine, software, removed, kept, plan=None):
    """The lines of the head of the report, and the warnings about the measurement."""
    lines, warnings = [], []
    if plan:
        warm = (f" after {plan.warmup} warm-up runs that do not count" if plan.warmup != 1 else " after 1 warm-up run that does not count") if plan.warmup else ""
        lines.append(f"Runs      {plan.runs}{warm}; the terminals in an order shuffled for each run, --seed {plan.seed} repeats it")
        if plan.settings:
            lines.append(f"Settings  {', '.join(plan.settings)} for Kudari")
    # One Kudari as a dict, or the builds to compare as (name, facts).
    builds = [("Kudari", kudari)] if isinstance(kudari, dict) else list(kudari or [])
    for label, kudari in builds:
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
        lines.append(f"{label:<9} {kudari['path']}: {', '.join(parts)}")
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


def environment(directory, binaries, local, removed, kept, plan=None):
    """Writes the head of the report to DIR/environment.txt and shows it. `binaries` is the
    Kudari measured, or the builds to compare as NAME=PATH."""
    named = [binary.split("=", 1) for binary in binaries if "=" in binary]
    if named:
        kudari = [(f"kudari@{name}", build(path, local)) for name, path in named]
    else:
        kudari = build(binaries[0], local) if binaries else None
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
            done = [(number, result[load]) for number, (_, result, _) in runs[terminal] if load in result and result[load].size]
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


def answers(runs, trips, terminals):
    """Per terminal, how long a query of the cursor took without output, and per load how long
    the terminal still worked after the last byte, until it answered: in milliseconds, the
    median and the largest over the runs, and how often it did not answer at all."""
    rows = []
    ran = [load for load in LOADS if any(load in result for measured in runs.values() for _, (_, result, _) in measured)]
    for terminal in terminals:
        taken = trips.get(terminal, [])
        cells = [terminal, f"{1000 * statistics.median(taken):.2f}" if taken else "-", f"{1000 * percentile(taken, 0.95):.2f}" if taken else "-"]
        found = False
        for load in ran:
            done = [result[load] for _, (_, result, _) in runs[terminal] if load in result and result[load].backlog is not None]
            if not done:
                cells.append("-")
                continue
            found = True
            backlog = [1000 * measured.backlog for measured in done if measured.answered]
            missing = sum(not measured.answered for measured in done)
            shown = f"{statistics.median(backlog):.1f} ({max(backlog):.1f})" if backlog else "-"
            cells.append(shown + (f" {missing} unanswered" if missing else ""))
        if found or taken:
            rows.append(cells)
    if not rows:
        return
    print("\nA load ends when the terminal answers a query of the cursor (DSR) after its last byte.")
    print("The query alone, without output, in milliseconds: median and 95th percentile of 100,")
    print("one right after the other, which keeps the computer awake as after a load.")
    print("Then per load, the milliseconds from the last byte written to that answer: the median")
    print("over the runs and, in brackets, the largest. Without an answer within 5 s, the load")
    print("ended with its last byte, and the report counts it as unanswered.")
    table(["terminal", "query ms", "95% ms"] + [f"{load} ms" for load in ran], rows)


def zooming(runs, terminals):
    """For the load resize: in a terminal that reports its frames, the milliseconds from each
    shortcut to the first frame after it and the longest gap between two frames meanwhile,
    over all runs; and for each terminal the shortcuts that went out."""
    rows = []
    for terminal in terminals:
        done = [result["resize"] for _, (_, result, _) in runs[terminal] if "resize" in result and result["resize"].keys]
        if not done:
            continue
        shown, gap = [], []
        for measured in done:
            times = sorted(found.time for found in measured.frames)
            for pressed, _ in measured.keys:
                later = [moment for moment in times if moment >= pressed]
                if later:
                    shown.append(1000 * (later[0] - pressed))
            gap.extend(gaps(measured.frames))
        cells = [terminal, str(sum(len(measured.keys) for measured in done))]
        cells += [f"{percentile(shown, 0.5):.1f}", f"{percentile(shown, 0.95):.1f}", f"{max(shown):.1f}"] if shown else ["-", "-", "-"]
        cells.append(f"{max(gap):.0f}" if gap else "-")
        rows.append(cells)
    if not rows:
        return
    print(f"\nresize: {DESCRIPTIONS['resize']}. For Kudari, the milliseconds from each shortcut")
    print("to the first frame after it, which shows the new size, and the longest gap between frames.")
    table(["terminal", "shortcuts", "to frame p50", "p95", "max", "longest gap ms"], rows)


def latency(runs, terminals):
    """For the loads that type: from each key to the terminal, and in a terminal that reports
    its frames to the first frame after the echo, in milliseconds over all runs."""
    for load in ("typing", "burst"):
        rows = []
        for terminal in terminals:
            done = [result[load] for _, (_, result, _) in runs[terminal] if load in result and result[load].keys]
            if not done:
                continue
            came = [1000 * (arrived - pressed) for measured in done for pressed, arrived in measured.keys]
            shown, per_key = [], []
            for measured in done:
                times = sorted(found.time for found in measured.frames)
                for pressed, arrived in measured.keys:
                    later = [moment for moment in times if moment >= arrived]
                    if later:
                        shown.append(1000 * (later[0] - pressed))
                if times:
                    per_key.append(len(times) / len(measured.keys))
            missing = sum(TYPED - len(measured.keys) for measured in done)
            cells = [terminal] + [f"{percentile(came, share):.2f}" for share in (0.5, 0.95, 0.99)]
            cells += [f"{percentile(shown, share):.1f}" for share in (0.5, 0.95)] if shown else ["-", "-"]
            cells += [f"{statistics.median(per_key):.2f}" if per_key else "-", str(missing)]
            rows.append((percentile(came, 0.5), cells))
        if not rows:
            continue
        print(f"\n{load}: {DESCRIPTIONS[load]}. Milliseconds from the press of each key in KWin to")
        print("the terminal (to the PTY), over all runs; for Kudari also to the first frame after the")
        print("echo, as Qt logs it when drawn, and the frames per key. Lost are keys that never came.")
        table(["terminal", "to PTY p50", "p95", "p99", "to frame p50", "p95", "frames/key", "lost"], [row for _, row in sorted(rows, key=lambda row: row[0])])


def replies(runs, terminals):
    """For the load queries: how many of its queries each terminal answered, over the runs."""
    rows = []
    for terminal in terminals:
        counts = [result["queries"].replies for _, (_, result, _) in runs[terminal] if "queries" in result and result["queries"].replies is not None]
        if counts:
            rows.append([terminal, f"{min(counts):,}", f"{max(counts):,}", f"{QUERIES:,}"])
    if rows:
        print("\nqueries: the answers that came, fewest and most over the runs, of the queries asked.")
        table(["terminal", "fewest", "most", "asked"], rows)


def asking(runs, terminals):
    """For the load query: the round trips of each query, alone and under output, in
    milliseconds over all runs, and how many got no answer."""
    rows = []
    for terminal in terminals:
        done = [result["query"].asked for _, (_, result, _) in runs[terminal] if "query" in result and result["query"].asked]
        for name, _, _ in ASKING:
            cells = [terminal, name]
            for state in ("alone", "busy"):
                taken = [time for asked in done for time in asked.get((name, state), [])]
                answered = [1000 * time for time in taken if time is not None]
                missing = len(taken) - len(answered)
                if answered:
                    cells += [f"{percentile(answered, 0.5):.2f}", f"{percentile(answered, 0.99):.2f}", f"{max(answered):.1f}"]
                else:
                    cells += ["-", "-", "-"]
                cells.append(str(missing))
            if done:
                rows.append(cells)
    if not rows:
        return
    print(f"\nquery: {ASKED} round trips of each query, alone and while the frames load goes to the")
    print("terminal, in milliseconds over all runs: median, 99th percentile and the longest, and how")
    print("many got no answer, as a terminal that does not answer the background at all.")
    table(["terminal", "query", "alone p50", "p99", "max", "none", "busy p50", "p99", "max", "none"], rows)


def processor(runs, terminals):
    """Per load, the CPU time of each terminal: all of it, as a share of a core, per MB of
    output, and its busiest groups of threads."""
    for load in LOADS:
        rows = []
        for terminal in terminals:
            done = [result[load] for _, (_, result, _) in runs[terminal] if load in result and result[load].cpu]
            if not done:
                continue
            totals = [sum(measured.cpu.values()) for measured in done]
            shares = [sum(measured.cpu.values()) / 10 / measured.seconds for measured in done]
            per_mb = [sum(measured.cpu.values()) / (measured.size / 1e6) for measured in done if measured.size]
            # A load of a fixed time costs per change of the screen.
            per_change = [sum(measured.cpu.values()) / FIXED_TIME[load] for measured in done] if FIXED_TIME.get(load) else []
            wakeups = [sum((measured.switches or {}).values()) / measured.seconds for measured in done]
            # Without output, how often a thread wakes up says more than its few milliseconds.
            idle = not per_mb
            values = {}
            for measured in done:
                for group, value in ((measured.switches or {}) if idle else measured.cpu).items():
                    values.setdefault(group, []).append(value / measured.seconds if idle else value)
            # Threads that hardly ran say nothing about the load.
            busy = [(group, found) for group, found in values.items() if statistics.median(found) >= 1]
            busiest = sorted(busy, key=lambda item: -statistics.median(item[1]))[:4]
            named = ", ".join(f"{group} {statistics.median(found):.0f}" for group, found in busiest) or "-"
            cost = f"{statistics.median(per_change):.2f}" if per_change else f"{statistics.median(per_mb):.1f}" if per_mb else "-"
            rows.append((statistics.median(per_mb) if per_mb else statistics.median(totals), [terminal, f"{statistics.median(totals):.0f}", f"{statistics.median(shares):.1f}" if idle else f"{statistics.median(shares):.0f}", cost, f"{statistics.median(wakeups):.0f}", named]))
        if not rows:
            continue
        print(f"\n{load}: CPU time of the terminal, median of the runs (all its threads, also those that draw)")
        busiest = "busiest threads, wakeups/s" if load in FIXED_TIME else "busiest threads, ms"
        cost = "ms per change" if FIXED_TIME.get(load) else "ms per MB"
        table(["terminal", "CPU ms", "% of a core", cost, "wakeups/s", busiest], [row for _, row in sorted(rows, key=lambda row: row[0])])


def memory(runs, terminals):
    """How much memory each terminal held after each load: private, and at the end also its
    resident and proportional share."""
    held = {}
    for terminal in terminals:
        for _, (_, result, _) in runs[terminal]:
            for load, measured in result.items():
                if measured.memory:
                    held.setdefault(terminal, {}).setdefault(load, []).append(measured.memory)
    if not held:
        return
    loads = [load for load in LOADS if any(load in after for after in held.values())]
    rows = []
    for terminal, after in held.items():
        last = after[[load for load in loads if load in after][-1]]
        cells = [f"{statistics.median(found[2] for found in after[load]) / 1024:.0f}" if load in after else "-" for load in loads]
        peaks = [found[3] for found in last if len(found) > 3 and found[3]]
        peak = f"{statistics.median(peaks) / 1024:.0f}" if peaks else "-"
        rows.append([terminal] + cells + [f"{statistics.median(found[0] for found in last) / 1024:.0f}", f"{statistics.median(found[1] for found in last) / 1024:.0f}", peak, *graphics(last)])
    print("\nMemory of each terminal in MB, median of the runs: private after each load (the history")
    print("fills from one load to the next), resident (RSS) and proportional (PSS) after the last, and")
    print("the most it was ever resident (peak), which a flood can drive up for a moment. VRAM is what")
    print("it held on the graphics card after the last load, GTT what in memory that the card reaches,")
    print("both from the fdinfo of its DRM devices; \"-\" for a terminal that draws in software or through")
    print("another process, as xterm through Xwayland.")
    table(["terminal"] + [f"after {load}" for load in loads] + ["RSS", "PSS", "peak", "VRAM", "GTT"], rows)


def graphics(samples):
    """The median VRAM and GTT of the samples of memory that have them, in MB, or dashes."""
    held = [found[4:6] for found in samples if len(found) > 5]
    if not held:
        return ["-", "-"]
    return [f"{statistics.median(found[index] for found in held) / 1024:.0f}" for index in (0, 1)]


def drawing(terminal, measured, detail):
    """For a terminal that reports its frames: how often and how fast it drew during each load."""
    rows, details = [], []
    for load in LOADS:
        done = [result[load] for _, (_, result, _) in measured if load in result and result[load].frames]
        if not done:
            continue
        rates = [len(load_run.frames) / load_run.window for load_run in done]
        # Without output, frames come only from the cursor, and keys 50 ms apart leave gaps
        # between theirs: neither is late.
        quiet = load in ("idle", "typing")
        frames = [found for load_run in done for found in load_run.frames]
        between = [gap for load_run in done for gap in gaps(load_run.frames)]
        blocked = [wait for load_run in done for wait in load_run.blocked]
        syncs = [found.sync for found in frames]
        late = f"{100 * sum(gap > LATE for gap in between) / len(between):.0f}" if between and not quiet else "-"
        costs = [sum(load_run.cpu.values()) / len(load_run.frames) for load_run in done if load_run.cpu]
        cost = f"{statistics.median(costs):.1f}" if costs else "-"
        share = f"{100 * sum(blocked) / 1000 / sum(load_run.window for load_run in done):.0f}" if blocked else "-"
        rows.append([load, f"{statistics.median(rates):.1f}", late, milliseconds(percentile(syncs, 0.5)), milliseconds(percentile(syncs, 0.95)), milliseconds(max(syncs)), share, cost])
        details.append((load, frames, between, blocked))
    loops = sorted({loop for _, (_, _, seen) in measured for loop in seen})
    print(f"\n{terminal}: frames that Qt Quick drew during each load (render loop {', '.join(loops)})")
    print(f"fps is the median over the runs; late are the gaps between frames of more than {LATE:.0f} ms, one at")
    print("60 Hz left out; sync is where Kudari paints, in whole milliseconds as Qt gives them, cut off;")
    print("GUI blocked is the share of the time the GUI thread waited for the render thread, at least;")
    print("CPU per frame is the CPU time of all threads during the load, divided by the frames.")
    table(["load", "fps", "late %", "sync p50", "sync p95", "sync max", "GUI blocked %", "CPU ms/frame"], rows)
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


def counters(terminal, measured):
    """For a build with the feature stats: what Kudari counted during each load."""
    rows = []
    for load in LOADS:
        done = [result[load].counters for _, (_, result, _) in measured if load in result and result[load].counters]
        if not done:
            continue

        def rate(name, done=done):
            return f"{statistics.median(found[name] / found['seconds'] for found in done if found['seconds'] > 0):.0f}"

        def pooled(name, done=done):
            width = max(len(found[name]["buckets"]) for found in done)
            return [sum(found[name]["buckets"][index] for found in done if index < len(found[name]["buckets"])) for index in range(width)]

        def p95(name):
            found = bucket_quantile(pooled(name), 0.95)
            return "-" if found is None else f"{found}"

        def mean(name, done=done):
            count = sum(found[name]["count"] for found in done)
            return f"{sum(found[name]['total_us'] for found in done) / count:.0f}" if count else "-"

        rows.append([load, rate("wakeups"), rate("refreshes"), rate("paints"), p95("lock_wait"), p95("lock_hold"), p95("queue"), mean("frame_build"), mean("frame_draw")])
    if not rows:
        return
    print(f"\n{terminal}: what the build with the feature stats counted during each load")
    print("Wakeups are those of the PTY thread for output; a second is the median over the runs, between")
    print("the lines of the counters around the load. p95 is the upper end of the bucket of powers of two")
    print("that holds 95 % of the times, over all runs, in microseconds: the wait for the lock of a")
    print("terminal, how long it was held, and the wait in the queue of the Qt thread. A frame is")
    print("built and drawn into its image; those are the means.")
    table(["load", "wakeups/s", "refreshes/s", "paints/s", "lock wait p95", "lock hold p95", "queue p95", "build µs", "draw µs"], rows)
    key_path(terminal, measured)


def key_path(terminal, measured):
    """For a build with the feature stats and the loads that type: the steps of the path of a
    key through Kudari, as its counters follow them."""
    for load in ("typing", "burst"):
        done = [result[load].counters for _, (_, result, _) in measured if load in result and result[load].counters]
        if not done or not any(found.get("key_total", {}).get("count") for found in done):
            continue
        rows = []
        for name, description in STEPS:
            count = sum(found.get(name, {}).get("count", 0) for found in done)
            if not count:
                rows.append([description, "0", "-", "-"])
                continue
            total = sum(found[name]["total_us"] for found in done)
            width = max(len(found[name]["buckets"]) for found in done)
            pooled = [sum(found[name]["buckets"][index] for found in done if index < len(found[name]["buckets"])) for index in range(width)]
            rows.append([description, str(count), f"{total / count:.0f}", str(bucket_quantile(pooled, 0.95))])
        print(f"\n{terminal}, {load}: the path of a key through Kudari, in microseconds over all runs. The")
        print("counters follow one key at a time, from its press in the terminal to the end of the next")
        print("paint of its lines; a key that comes while one is on its way is not followed, so a burst")
        print("has few. Each step counts from the one before; the mean, and the upper end of the bucket")
        print("of powers of two that holds 95 %. From the paint to the screen, see the frames above.")
        table(["step", "keys", "mean µs", "p95 µs"], rows)


def ratio_interval(base, other, seed=SEED, rounds=2000):
    """A 95 % bootstrap interval of the ratio of the medians, `other` over `base`."""
    rng = random.Random(seed)
    ratios = sorted(statistics.median(rng.choices(other, k=len(other))) / statistics.median(rng.choices(base, k=len(base))) for _ in range(rounds))
    return ratios[int(0.025 * rounds)], ratios[int(0.975 * rounds) - 1]


def verdict(low, high, words=("faster", "slower")):
    """Faster or slower, or the words given, only where the interval of the ratio leaves out 1."""
    if high < 1:
        return words[0]
    if low > 1:
        return words[1]
    return "no difference"


def compare(runs, builds):
    """Each build of Kudari against the first, load by load: the time and the CPU time."""
    base = builds[0]
    print(f"\nThe builds of Kudari against {base}: the median of each, the ratio of the medians and a")
    print("95 % bootstrap interval of it (seed 1). A build is faster or slower only where the interval")
    print("leaves out 1; with few runs, the interval is wide and small changes are no difference.")
    measures = (
        ("seconds per load", lambda measured: measured.seconds, ("faster", "slower"), 2),
        ("CPU ms per load", lambda measured: sum(measured.cpu.values()) if measured.cpu else None, ("less", "more"), 0),
    )
    for what, value, words, precision in measures:
        rows = []
        for load in LOADS:
            # A load of a fixed time takes it in any build.
            if load in FIXED_TIME and what.startswith("seconds"):
                continue
            for other in builds[1:]:
                found = [[value(result[load]) for _, (_, result, _) in runs[build] if load in result and (load in FIXED_TIME or result[load].size)] for build in (base, other)]
                found = [[number for number in numbers if number] for numbers in found]
                if len(found[0]) < 2 or len(found[1]) < 2:
                    continue
                low, high = ratio_interval(*found)
                ratio = statistics.median(found[1]) / statistics.median(found[0])
                rows.append([load, other, f"{statistics.median(found[0]):.{precision}f}", f"{statistics.median(found[1]):.{precision}f}", f"{ratio:.3f}", f"{low:.3f}-{high:.3f}", verdict(low, high, words)])
        if rows:
            print(f"\n{what}")
            table(["load", "build", base, "build", "ratio", "95% ratio", "verdict"], rows)


def profiles(directory):
    """For a run with --perf: the command of `perf report` for each load of each run, by the
    times of its start and end on the clock of `perf`."""
    found = []
    for path in sorted(directory.glob("*-*.perf")):
        terminal, run = path.stem.rsplit("-", 1)
        data = directory / f"perf-{terminal}-{run}.data"
        if not run.isdigit() or not data.exists():
            continue
        for line in path.read_text().splitlines():
            parts = line.split()
            if len(parts) == 3:
                found.append((terminal, run, parts[0], f"perf report -i {data} --time {parts[1]},{parts[2]} --sort comm,dso,sym"))
    if not found:
        return
    print("\nProfiles of perf, one for each load of each run; --sort sym --comm 'PTY reader' and the like")
    print("show one thread, and perf report --no-children shows where the time itself goes.")
    for terminal, run, load, command in found:
        print(f"{terminal} run {run}, {load}: {command}")


def ranged(whole, width):
    """Whole milliseconds cut off, `width` of them added up, as the range they stand for."""
    return milliseconds(whole) if width == 1 else f"{whole}-{whole + width}"


def report(directory, order, detail=False):
    head = directory / "environment.txt"
    if head.exists():
        print()
        print(head.read_text(), end="")
    runs, warmups, taken = {}, 0, {}
    for path in sorted(directory.glob("*-*.txt")):
        terminal, run = path.stem.rsplit("-", 1)
        # The results of a run, not the screens of the frames load; warm-up runs do not count.
        if run.isdigit():
            runs.setdefault(terminal, []).append((run, measure(directory, path.stem)))
            taken.setdefault(terminal, []).extend(trips(directory, path.stem))
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
    latency(runs, terminals)
    zooming(runs, terminals)
    replies(runs, terminals)
    asking(runs, terminals)
    answers(runs, taken, terminals)
    processor(runs, terminals)
    memory(runs, terminals)
    # Only Qt Quick reports its frames: those of Kudari.
    for terminal in terminals:
        if terminal.split("@")[0] == "kudari" and not any(measured.frames for _, (_, result, _) in runs[terminal] for measured in result.values()):
            print(f"\n{terminal}: no frame timings in its logs; did Qt honour QSG_RENDER_TIMING, and in which render loop?")
    for terminal in terminals:
        if any(measured.frames for _, (_, result, _) in runs[terminal] for measured in result.values()):
            drawing(terminal, runs[terminal], detail)
        counters(terminal, runs[terminal])
    builds = [terminal for terminal in terminals if terminal.startswith("kudari@")]
    if len(builds) > 1:
        compare(runs, builds)
    profiles(directory)
    gone = [(path.stem, load) for path in sorted(directory.glob("*-*.txt")) if path.stem.rsplit("-", 1)[1].isdigit() and (load := ended(directory, path.stem))]
    if gone:
        print("\nTerminals that ended or hung during a load, which then and after counts for nothing:")
        for name, load in gone:
            terminal, run = name.rsplit("-", 1)
            print(f"{terminal} in run {run}, during {load}")

# The scenarios of `tools/bench.sh --scenarios` that wait, in the order of the report, with
# what each of them shows.
WAITING = (
    ("never-shown", "started, the window never shown"),
    ("shown", "shown, nothing happens"),
    ("hidden", "hidden after it was shown"),
    ("blinking", "shown, with a blinking cursor"),
    ("rested", "the blinking cursor after 15 s, at rest"),
    ("fading", "shown, with a fading cursor"),
    ("output-behind", "ten lines a second in a tab in the background"),
    ("output-hidden", "the same, with the window hidden"),
    ("flood-behind", "endless output in a tab in the background"),
)
MEMORY = (
    ("memory-1", "1 tab"),
    ("memory-5", "5 tabs"),
    ("memory-10", "10 tabs"),
    ("memory-hidden", "10 tabs, window hidden"),
    ("memory-closed", "closed to 1"),
)
# The lines of history that the scenario history fills, by writing a screen more than that.
HISTORY = 20_000
# The phases that Kudari marks through KUDARI_TIMING while it starts, in their order: the
# controller starts its first session while QML loads.
PHASES = ("main", "application", "bus", "started", "qml", "ready")
TIMING = re.compile(r"^kudari-timing (\S+) (\d+(?:\.\d+)?)$")


def scenario_run(directory, run):
    """One run of the scenarios: its events by kind and name, the frames and the marks of each
    Kudari by name, and what the samples of its threads found, by scenario."""
    events = {}
    for line in (directory / f"events-{run}.txt").read_text().splitlines():
        parts = line.split()
        if len(parts) == 3:
            events.setdefault((parts[0], parts[1]), []).append(float(parts[2]))
    drawn, marks, used, woken, memory = {}, {}, {}, {}, {}
    for log in directory.glob(f"*-{run}.log"):
        name = log.stem.rsplit("-", 1)[0]
        for line in log.read_text(errors="replace").splitlines():
            if found := frame(line):
                drawn.setdefault(name, []).append(found.time)
            elif found := TIMING.match(line):
                marks.setdefault(name, []).append((found[1], float(found[2])))
    for path in directory.glob(f"*-{run}.cpu"):
        taken, switched, after = threads(path)
        used.update(taken)
        woken.update(switched)
        memory.update(after)
    return events, drawn, marks, used, woken, memory


def first_after(times, moment, within=2.0):
    """The first of `times` after `moment` and within `within` seconds of it, or None."""
    later = [time for time in times if moment <= time <= moment + within]
    return min(later) if later else None


def summary(values, unit=1000):
    """Median, the value below which 95 % lie, and the largest of the values, in milliseconds."""
    if not values:
        return ["-", "-", "-", "0"]
    shown = [f"{value * unit:.1f}" for value in (statistics.median(values), percentile(values, 0.95), max(values))]
    return shown + [str(len(values))]


def scenarios(directory):
    """The report of `tools/bench.sh --scenarios`."""
    head = directory / "environment.txt"
    if head.exists():
        print()
        print(head.read_text(), end="")
    runs = sorted((path.stem.split("-", 1)[1] for path in directory.glob("events-*.txt")), key=lambda run: (not run.isdigit(), run))
    runs = [run for run in runs if run.isdigit()]
    if not runs:
        print("\nNo run of the scenarios finished; the logs of Kudari tell why.")
        return
    starts, phases, first_shows, shows, toggles, seconds, versions = [], {}, [], [], [], [], []
    waiting, memory, grown = {}, {}, []
    for run in runs:
        events, drawn, marks, used, woken, sizes = scenario_run(directory, run)
        spawn = events.get(("spawn", "main"), [None])[0]
        bus = events.get(("bus", "main"), [None])[0]
        if spawn is not None and bus is not None:
            frame_at = first_after(drawn.get("main", []), spawn, within=30)
            starts.append((bus - spawn, frame_at - spawn if frame_at else None))
            for phase, moment in marks.get("main", []):
                if phase in PHASES:
                    phases.setdefault(phase, []).append(moment - spawn)
        for name, into in (("hidden", first_shows), ("main", shows)):
            for moment in events.get(("show", name), []):
                if (found := first_after(drawn.get(name, []), moment)) is not None:
                    into.append(found - moment)
                toggled = first_after([time for phase, time in marks.get(name, []) if phase == "toggle"], moment)
                if toggled is not None:
                    toggles.append(toggled - moment)
        for kind, into in (("second", seconds), ("version", versions)):
            into.extend(end - start for start, end in zip(events.get((f"{kind}-start", "main"), []), events.get((f"{kind}-end", "main"), []), strict=False))
        for scenario, _ in WAITING:
            start = events.get(("idle", scenario), [None])[0]
            end = events.get(("idle-end", scenario), [None])[0]
            if start is None or end is None or scenario not in used:
                continue
            length = end - start
            frames_meanwhile = sum(start <= time <= end for times in drawn.values() for time in times)
            per_thread = {group: used_ms / length for group, used_ms in used[scenario].items()}
            waiting.setdefault(scenario, []).append((sum(used[scenario].values()) / length, sum((woken.get(scenario) or {}).values()) / length, frames_meanwhile / length, per_thread))
        for label, _ in MEMORY:
            if label in sizes:
                memory.setdefault(label, []).append(sizes[label])
        size = (content(directory / f"size-{run}.txt") or "").split()
        if "history-empty" in sizes and "history-full" in sizes and len(size) == 2 and size[1].isdigit():
            grown.append((sizes["history-full"][2] - sizes["history-empty"][2], int(size[1])))
    print(f"\n{len(runs)} runs of the scenarios.")
    print("\nStart: from the start of the process, in ms; median and the slowest run.")
    rows = [["name on the bus", *ranges([bus for bus, _ in starts])], ["first frame", *ranges([shown for _, shown in starts if shown is not None])]]
    rows += [[f"phase {phase}", *ranges(phases[phase])] for phase in PHASES if phase in phases]
    table(["", "median", "slowest", "runs"], rows)
    print("\nShowing the window: from toggleWindowState over D-Bus to the next frame, and to the")
    print("call reaching Kudari, in ms.")
    table(["", "median", "p95", "max", "times"], [["first time", *summary(first_shows)], ["the times after", *summary(shows)], ["call received", *summary(toggles)]])
    print("\nA second start, which only toggles the running Kudari, and kudari --version, to their end, in ms.")
    table(["", "median", "p95", "max", "times"], [["second start", *summary(seconds)], ["--version", *summary(versions)]])
    print("\nWaiting, ten seconds each: CPU time of all threads, how often they went to sleep or were")
    print("put aside, the frames that Qt Quick drew, and the three busiest threads; the median of the")
    print("runs, in milliseconds a second.")
    rows = []
    for scenario, description in WAITING:
        if found := waiting.get(scenario):
            groups = {group for *_, per_thread in found for group in per_thread}
            busiest = sorted(((statistics.median(per_thread.get(group, 0.0) for *_, per_thread in found), group) for group in groups), reverse=True)[:3]
            threads = ", ".join(f"{group} {used_ms:.2f}" for used_ms, group in busiest)
            rows.append([description, f"{statistics.median(found_[0] for found_ in found):.2f}", f"{statistics.median(found_[1] for found_ in found):.0f}", f"{statistics.median(found_[2] for found_ in found):.1f}", threads])
    table(["", "CPU ms/s", "wakeups/s", "frames/s", "busiest threads"], rows)
    print("\nMemory in MB, median of the runs: private, resident (RSS) and proportional (PSS), and")
    print("on the graphics card (VRAM) and in memory that the card reaches (GTT), from the fdinfo of")
    print("the DRM devices of Kudari; \"-\" when it draws in software.")
    rows = []
    for label, description in MEMORY:
        if found := memory.get(label):
            rows.append([description, *(f"{statistics.median(size[index] for size in found) / 1024:.0f}" for index in (2, 0, 1)), *graphics(found)])
    table(["", "private", "RSS", "PSS", "VRAM", "GTT"], rows)
    if grown:
        print(f"\nThe history: what filling a history of {HISTORY} lines added to the private memory of")
        print("Kudari, per 1000 lines and per cell of a line, median of the runs; the columns are those of")
        print("its terminal.")
        per_thousand = [added / (HISTORY / 1000) for added, _ in grown]
        per_cell = [added * 1024 / (HISTORY * columns) for added, columns in grown]
        columns = sorted({columns for _, columns in grown})
        table(["columns", "MB per 1000 lines", "bytes per cell", "runs"], [[", ".join(map(str, columns)), f"{statistics.median(per_thousand) / 1024:.2f}", f"{statistics.median(per_cell):.1f}", str(len(grown))]])


def ranges(values):
    """Median and largest of the values, in milliseconds, and how many there are."""
    if not values:
        return ["-", "-", "0"]
    return [f"{statistics.median(values) * 1000:.1f}", f"{max(values) * 1000:.1f}", str(len(values))]


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("action", choices=("data", "frames", "environment", "order", "cpu", "report", "scenarios", "workload"))
    parser.add_argument("directory", type=Path)
    parser.add_argument("rest", nargs="*", help="rows and columns for frames, the Kudari measured or the builds to compare as NAME=PATH for environment, terminals for report")
    parser.add_argument("--local", help="for environment: the release build of the checkout, to warn when it is newer")
    parser.add_argument("--variables", default="", help="for environment: the variables of Qt, Mesa and the like that were set, with commas")
    parser.add_argument("--keep-env", action="store_true", help="for environment: those variables were kept")
    parser.add_argument("--runs", type=int, default=0, help="for environment: the runs that count")
    parser.add_argument("--warmup", type=int, default=0, help="for environment: the warm-up runs before them")
    parser.add_argument("--seed", default="", help="for environment and order: the seed of the order of the terminals")
    parser.add_argument("--detail", action="store_true", help="for report: the times of the frames in detail")
    arguments = parser.parse_args()
    if arguments.action == "environment":
        removed = [name for name in arguments.variables.split(",") if name]
        settings = tuple((content(arguments.directory / "settings") or "").splitlines())
        plan = Plan(arguments.runs, arguments.warmup, arguments.seed, settings) if arguments.runs else None
        environment(arguments.directory, arguments.rest, arguments.local, removed, arguments.keep_env, plan)
    elif arguments.action == "cpu":
        if len(arguments.rest) != 3 or not arguments.rest[0].isdigit() or arguments.rest[2] not in ("start", "end"):
            parser.error("cpu needs the file, the process, the load and start or end")
        cpu(arguments.directory, *arguments.rest)
    elif arguments.action == "order":
        # The directory stands for the run here, to keep the arguments in one place.
        print(" ".join(order(arguments.seed, str(arguments.directory), arguments.rest)))
    elif arguments.action == "data":
        data(arguments.directory)
    elif arguments.action == "frames":
        if len(arguments.rest) != 2 or not all(part.isdigit() for part in arguments.rest):
            parser.error("frames needs the rows and the columns")
        frames(arguments.directory, *map(int, arguments.rest))
        boxes(arguments.directory, *map(int, arguments.rest))
    elif arguments.action == "scenarios":
        scenarios(arguments.directory)
    elif arguments.action == "workload":
        workload(arguments.directory, os.environ)
    else:
        report(arguments.directory, arguments.rest, arguments.detail)


if __name__ == "__main__":
    main()
