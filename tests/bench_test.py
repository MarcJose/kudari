#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
"""The report of tools/bench.py on runs made up here: tests/check.sh runs it."""

import contextlib
import importlib.util
import io
import sys
import tempfile
import unittest
from pathlib import Path

TOOLS = Path(__file__).resolve().parent.parent / "tools"
spec = importlib.util.spec_from_file_location("bench", TOOLS / "bench.py")
bench = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bench)

START = "2026-01-01T10:00:00.000"


def stamp(seconds):
    return f"2026-01-01T10:00:{seconds:06.3f}"


def threaded(seconds, sync):
    return f"{stamp(seconds)} qt.scenegraph.time.renderloop [render thread 0x1] syncAndRender: frame rendered in 3ms, sync={sync}, render=1, swap=1"


class Report(unittest.TestCase):
    def setUp(self):
        self.directory = Path(tempfile.mkdtemp())
        self.addCleanup(lambda: [path.unlink() for path in self.directory.iterdir()] and self.directory.rmdir())
        for name in ("plain.txt", "color.txt", "frames-40x100.txt"):
            (self.directory / name).write_text("x" * 1000)

    def run_of(self, name, log_lines, loads=(("plain", 0.0, 0.2),)):
        lines = ["size 40 100"] + [f"{load} {stamp(start)} {stamp(end)}" for load, start, end in loads] + ["done"]
        (self.directory / f"{name}.txt").write_text("\n".join(lines) + "\n")
        (self.directory / f"{name}.log").write_text("\n".join(log_lines) + "\n")

    def report(self, *terminals):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            bench.report(self.directory, list(terminals))
        return output.getvalue()

    def frames_row(self, text, load):
        part = text.split("frames that Qt Quick drew")[1]
        return next(line.split() for line in part.splitlines() if line.startswith(load + " "))

    def test_the_rate_counts_frames_over_the_seconds_they_were_counted_in(self):
        # 12 frames from the start of a load of 0.2 s to 50 ms after it: 48 a second.
        self.run_of("kudari-1", [threaded(0.02 * index, 1) for index in range(12)])
        self.assertEqual(self.frames_row(self.report("kudari"), "plain")[1], "48.0")

    def test_milliseconds_cut_off_by_qt_count_from_their_middle(self):
        self.run_of("kudari-1", [threaded(0.01, 0), threaded(0.02, 1), threaded(0.03, 2)])
        text = self.report("kudari")
        row = self.frames_row(text, "plain")
        # The middles of 0, 1 and 2 are 0.5, 1.5 and 2.5.
        self.assertEqual(row[4:], ["1.5", "<1", "2-3"])
        self.assertIn("whole milliseconds, cut off", text)

    def test_every_render_loop_of_qt_gives_its_frames(self):
        lines = [
            f"{stamp(0.01)} qt.scenegraph.time.renderloop [gui thread 0x1] renderWindow: frame rendered in 5ms, polish=0, sync=1, render=2, swap=2",
            f"{stamp(0.02)} qt.scenegraph.time.renderloop Frame rendered with 'software' renderloop in 4ms, sync=2, render=1, swap=1",
            threaded(0.03, 3),
        ]
        self.run_of("kudari-1", lines)
        text = self.report("kudari")
        self.assertIn("render loop basic, software, threaded", text)
        self.assertEqual(self.frames_row(text, "plain")[4], "2.5")

    def test_kudari_without_frames_is_named(self):
        self.run_of("kudari-1", ["nothing of Qt Quick"])
        self.assertIn("kudari: no frame timings in its logs", self.report("kudari"))

    def test_the_head_comes_first(self):
        (self.directory / "environment.txt").write_text("Computer  Some CPU\n")
        self.run_of("kudari-1", [threaded(0.01, 1)])
        text = self.report("kudari")
        self.assertLess(text.index("Computer  Some CPU"), text.index("runs per terminal"))

    def test_no_run_at_all_is_said_plainly(self):
        self.assertIn("No run finished", self.report("kudari"))


MACHINE = {
    "kernel": "7.0",
    "threads": 16,
    "load": 0.2,
    "processor": "Some CPU",
    "driver": "amd-pstate-epp",
    "governor": "powersave",
    "epp": "performance",
    "boost": True,
    "power profile": "performance",
    "on battery": False,
}
SOFTWARE = {"Qt": "6.12.0", "KWin": "6.8.3", "Mesa": None}
KUDARI = {
    "path": "/usr/bin/kudari",
    "sha256": "ab" * 32,
    "modified": 0.0,
    "version": "kudari 0.2.0",
    "build id": "cd" * 20,
    "compilers": ["rustc 1.100.0", "GCC 16.2.1"],
    "debug info": False,
    "package": "kudari-git 0.2.0-1",
    "makepkg": ("-C force-frame-pointers=yes", "-O2"),
}


class Environment(unittest.TestCase):
    def describe(self, kudari=None, removed=(), kept=False, **machine):
        return bench.describe(kudari, {**MACHINE, **machine}, SOFTWARE, list(removed), kept)

    def warnings(self, *arguments, **machine):
        return [line for line in self.describe(*arguments, **machine) if line.startswith("Warning:")]

    def test_a_quiet_computer_set_for_performance_gives_no_warning(self):
        self.assertEqual(self.warnings(), [])

    def test_the_build_measured_is_told_apart_by_hash_origin_and_flags(self):
        lines = self.describe(KUDARI)
        self.assertIn("package kudari-git 0.2.0-1", lines[0])
        self.assertIn("sha256 abababababababab", lines[0])
        self.assertIn("without debug info", lines[0])
        self.assertIn("RUSTFLAGS=-C force-frame-pointers=yes CXXFLAGS=-O2", lines[1])
        self.assertIn("Mesa unknown", "\n".join(lines))

    def test_a_newer_local_build_is_named(self):
        warnings = self.warnings({**KUDARI, "newer local build": "/src/target/release/kudari"})
        self.assertEqual(len(warnings), 1)
        self.assertIn("/src/target/release/kudari is newer", warnings[0])

    def test_load_battery_and_a_slow_clock_are_warned_about(self):
        warnings = self.warnings(load=1.5, governor="powersave", epp="balance_performance", **{"on battery": True})
        self.assertEqual(len(warnings), 3)
        self.assertIn("powerprofilesctl launch -p performance", warnings[2])

    def test_the_variables_say_whether_they_were_removed(self):
        self.assertIn("QT_SCALE_FACTOR, LD_PRELOAD: removed", "\n".join(self.describe(removed=["QT_SCALE_FACTOR", "LD_PRELOAD"])))
        self.assertIn("kept, with --keep-env", "\n".join(self.describe(removed=["QT_SCALE_FACTOR"], kept=True)))


if __name__ == "__main__":
    sys.exit(unittest.main())
