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


if __name__ == "__main__":
    sys.exit(unittest.main())
