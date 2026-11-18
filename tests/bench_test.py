#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
"""The report of tools/bench.py on runs made up here: tests/check.sh runs it."""

import contextlib
import importlib.util
import io
import json
import os
import sys
import tempfile
import time
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

    def test_milliseconds_cut_off_by_qt_show_as_the_range_they_stand_for(self):
        self.run_of("kudari-1", [threaded(0.01, 0), threaded(0.02, 1), threaded(0.03, 2)])
        text = self.report("kudari")
        # sync p50, p95 and the slowest.
        self.assertEqual(self.frames_row(text, "plain")[3:6], ["1-2", "2-3", "2-3"])
        self.assertIn("whole milliseconds as Qt gives them, cut off", text)

    def test_every_render_loop_of_qt_gives_its_frames(self):
        lines = [
            f"{stamp(0.01)} qt.scenegraph.time.renderloop [gui thread 0x1] renderWindow: frame rendered in 5ms, polish=0, sync=1, render=2, swap=2",
            f"{stamp(0.02)} qt.scenegraph.time.renderloop Frame rendered with 'software' renderloop in 4ms, sync=2, render=1, swap=1",
            threaded(0.03, 3),
        ]
        self.run_of("kudari-1", lines)
        text = self.report("kudari")
        self.assertIn("render loop basic, software, threaded", text)
        self.assertEqual(self.frames_row(text, "plain")[3], "2-3")

    def test_kudari_without_frames_is_named(self):
        self.run_of("kudari-1", ["nothing of Qt Quick"])
        self.assertIn("kudari: no frame timings in its logs", self.report("kudari"))

    def test_the_head_comes_first(self):
        (self.directory / "environment.txt").write_text("Computer  Some CPU\n")
        self.run_of("kudari-1", [threaded(0.01, 1)])
        text = self.report("kudari")
        self.assertLess(text.index("Computer  Some CPU"), text.index("runs per terminal"))

    def load_row(self, text, load, terminal):
        part = text.split(f"\n{load}: ")[1]
        return next(line.split() for line in part.splitlines() if line.startswith(terminal + " "))

    def test_the_median_counts_and_warm_up_runs_do_not(self):
        for run, seconds in (("1", 0.20), ("2", 0.22), ("3", 0.21), ("w1", 0.90)):
            self.run_of(f"kudari-{run}", [], (("plain", 0.0, seconds),))
        text = self.report("kudari")
        self.assertIn("3 runs per terminal after 1 warm-up run that does not count", text)
        row = self.load_row(text, "plain", "kudari")
        # median, MAD in percent, min, max, runs, outliers
        self.assertEqual([row[2], row[3], row[5], row[6], row[8], row[9]], ["0.21", "4.8", "0.20", "0.22", "3", "-"])

    def test_a_run_far_from_the_others_is_named(self):
        for run, seconds in (("1", 0.20), ("2", 0.20), ("3", 0.202), ("4", 0.20), ("5", 0.40)):
            self.run_of(f"kudari-{run}", [], (("plain", 0.0, seconds),))
        self.assertEqual(self.load_row(self.report("kudari"), "plain", "kudari")[9], "5")

    def test_the_rate_is_the_median_of_the_runs(self):
        # 5, 10 and 25 frames in 0.25 s each: 20, 40 and 100 a second.
        for run, count in (("1", 5), ("2", 10), ("3", 25)):
            self.run_of(f"kudari-{run}", [threaded(0.008 * index, 1) for index in range(count)])
        self.assertEqual(self.frames_row(self.report("kudari"), "plain")[1], "40.0")

    def test_frames_far_apart_count_as_late(self):
        # Gaps of 17, 17 and 40 ms: one of three is late at 60 Hz.
        self.run_of("kudari-1", [threaded(time, 1) for time in (0.010, 0.027, 0.044, 0.084)])
        self.assertEqual(self.frames_row(self.report("kudari"), "plain")[2], "33")

    def test_the_wait_of_the_gui_thread_is_a_share_of_the_load(self):
        # 125 ms of 0.25 s (the load and its tail).
        lines = [threaded(0.05, 1)] + [f"{stamp(0.01 * index)} qt.scenegraph.time.renderloop [window 0x1][gui thread] Frame prepared, polish=0 ms, lock=0 ms, blockedForSync=25 ms, animations=0 ms" for index in range(1, 6)]
        self.run_of("kudari-1", lines)
        self.assertEqual(self.frames_row(self.report("kudari"), "plain")[6], "50")

    def test_the_detail_gives_percentiles_gaps_and_budgets(self):
        self.run_of("kudari-1", [threaded(0.017 * index, 9) for index in range(10)])
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            bench.report(self.directory, ["kudari"], detail=True)
        text = output.getvalue()
        self.assertIn("kudari, plain: 10 frames", text)
        self.assertIn("gap", text)
        # sync 9 and render 1, cut off: at least 10 ms, over the budget of 120 and 144 Hz only.
        self.assertIn("at least 0 % / 100 % / 100 %", text)

    def cpu_of(self, name, load, *threads, switches=None, memory=None):
        """Threads as (their ID, name, milliseconds at the start, at the end); `switches` adds
        that many to each thread between start and end; `memory` is RSS, PSS and private in kB."""
        lines = []
        for tid, thread, start, end in threads:
            if start is not None:
                lines.append(f"{load}\tstart\t{tid}\t{thread}\t{int(start * 1e6)}\t1\t0")
            if end is not None:
                lines.append(f"{load}\tend\t{tid}\t{thread}\t{int(end * 1e6)}\t{1 + (switches or 0)}\t0")
        if memory:
            lines.append(f"{load}\tmemory\t" + "\t".join(str(size) for size in memory))
        path = self.directory / f"{name}.cpu"
        with open(path, "a") as file:
            file.write("\n".join(lines) + "\n")

    def test_the_cpu_time_of_a_load_is_per_thread_between_its_start_and_end(self):
        self.run_of("kudari-1", [threaded(0.01 * index, 1) for index in range(10)])
        self.cpu_of(
            "kudari-1",
            "plain",
            (1, "main", 100, 104),
            (2, "PTY reader", 50, 150),
            # Two threads of a pool count together; one that began meanwhile counts from 0.
            (3, "Thread (pooled)", 10, 20),
            (4, "Thread (pooled)", None, 30),
            (5, "QSGRenderThread", 0, 0.4),
            # Threads numbered at the end count together as well.
            (6, "gpu:7", 0, 3),
            (7, "gpu:8", 0, 3),
        )
        text = self.report("kudari")
        part = text.split("plain: CPU time of the terminal")[1].splitlines()
        line = next(entry for entry in part if entry.startswith("kudari "))
        # 4 + 100 + 40 + 0.4 + 6 ms in 0.2 s of load: 75 % of a core.
        self.assertIn(" 150 ", line)
        self.assertIn(" 75 ", line)
        self.assertIn("PTY reader 100, Thread (pooled) 40, gpu 6, main 4", line)
        self.assertNotIn("QSGRenderThread", line)
        # 150.4 ms over 10 frames.
        self.assertEqual(self.frames_row(text, "plain")[7], "15.0")

    def test_the_threads_of_a_process_are_read_from_proc(self):
        path = self.directory / "self.cpu"
        bench.cpu(path, os.getpid(), "plain", "start")
        fields = [line.split("\t") for line in path.read_text().splitlines()]
        self.assertIn(["plain", "start", str(os.getpid()), "main"], [entry[:4] for entry in fields])
        self.assertTrue(all(entry[4].isdigit() for entry in fields))
        # At the end, also its memory.
        bench.cpu(path, os.getpid(), "plain", "end")
        memory = [line.split("\t") for line in path.read_text().splitlines() if "\tmemory\t" in line]
        self.assertEqual(len(memory), 1)
        rss, pss, private, peak = map(int, memory[0][2:])
        self.assertGreater(rss, 0)
        # Python shares its libraries with other processes, so not all of it is private.
        self.assertLess(private, rss)
        # The most it was ever resident is at least what it is now.
        self.assertGreaterEqual(peak, rss)

    def test_without_output_the_wakeups_count_and_no_throughput(self):
        self.run_of("kudari-1", [threaded(0.5 * index, 1) for index in range(4)], (("idle", 0.0, 2.0),))
        # 30 and 10 switches in 2 s.
        self.cpu_of("kudari-1", "idle", (1, "main", 0, 2), (2, "QSGRenderThread", 0, 1), switches=None)
        self.cpu_of("kudari-1", "idle", (3, "Thread (pooled)", 0, 0.5), switches=30)
        self.cpu_of("kudari-1", "idle", (4, "PTY reader", 0, 0), switches=10)
        text = self.report("kudari")
        self.assertNotIn("\nidle: 10 seconds", text)
        line = next(entry for entry in text.split("idle: CPU time of the terminal")[1].splitlines() if entry.startswith("kudari "))
        # 3.5 ms in 2 s; 40 switches and the 0 of the others in 2 s; the busiest by how often they woke up.
        self.assertEqual(line.split()[1:5], ["4", "0.2", "-", "20"])
        self.assertIn("Thread (pooled) 15, PTY reader 5", line)
        self.assertIn("wakeups/s", text.split("idle: CPU time of the terminal")[1].splitlines()[1])
        # The cursor frames of an idle terminal are not late.
        self.assertEqual(self.frames_row(text, "idle")[2], "-")

    def test_the_memory_after_each_load_and_at_the_end(self):
        for run, private in (("1", 90 * 1024), ("2", 100 * 1024), ("3", 95 * 1024)):
            self.run_of(f"kudari-{run}", [], (("plain", 0.0, 0.2), ("idle", 1.0, 2.0)))
            self.cpu_of(f"kudari-{run}", "plain", (1, "main", 0, 1), memory=(300 * 1024, 110 * 1024, private))
            self.cpu_of(f"kudari-{run}", "idle", (1, "main", 0, 1), memory=(310 * 1024, 120 * 1024, private + 1024))
        text = self.report("kudari")
        row = next(line.split() for line in text.split("Memory of each terminal")[1].splitlines() if line.startswith("kudari "))
        # Runs before the peak was noted have none.
        self.assertEqual(row, ["kudari", "95", "96", "310", "120", "-"])
        self.assertIn("after plain  after idle", text)

    def test_the_peak_of_memory_is_the_most_ever_resident(self):
        for run, peak in (("1", 400 * 1024), ("2", 500 * 1024), ("3", 450 * 1024)):
            self.run_of(f"kudari-{run}", [], (("bells", 0.0, 0.2),))
            self.cpu_of(f"kudari-{run}", "bells", (1, "main", 0, 1), memory=(300 * 1024, 110 * 1024, 90 * 1024, peak))
        text = self.report("kudari")
        row = next(line.split() for line in text.split("Memory of each terminal")[1].splitlines() if line.startswith("kudari "))
        self.assertEqual(row[-1], "450")

    def test_the_answers_to_the_queries_are_counted(self):
        for run, answered in (("1", bench.QUERIES), ("2", bench.QUERIES - 5)):
            self.monotonic_run(f"kudari-{run}", [], (("queries", 0.0, 1.0, 0.9, True),))
            with open(self.directory / f"kudari-{run}.txt", "a") as file:
                file.write(f"replies queries {answered}\n")
        row = next(line.split() for line in self.report("kudari").split("queries: the answers that came")[1].splitlines() if line.startswith("kudari "))
        self.assertEqual(row[1:], ["99,995", "100,000", "100,000"])

    def stats(self, seconds, wakeups, paints, waits, builds):
        """A line of the counters of a stats build at `seconds` of the run."""
        times = {name: {"count": 0, "total_us": 0, "buckets": [0] * 32} for name, _ in bench.TIMED}
        times["lock_wait"] = {"count": sum(waits), "total_us": 0, "buckets": waits + [0] * (32 - len(waits))}
        times["frame_build"] = {"count": builds[0], "total_us": builds[1], "buckets": [0] * 32}
        counters = {"wakeups": wakeups, "reports": 0, "refreshes": wakeups // 2, "paints": paints, **times}
        return f"kudari-stats {bench.when(stamp(seconds)):.3f} {json.dumps(counters)}"

    def test_the_counters_of_a_stats_build_count_per_load_between_their_lines(self):
        quiet = [0] * 11
        busy = [0, 0, 18, 0, 0, 0, 0, 0, 0, 0, 2]
        self.run_of("kudari-1", [
            self.stats(0.0, 10, {"1": 3}, quiet, (0, 0)),
            # Within the load: not a line around it.
            self.stats(0.1, 30, {"1": 13}, quiet, (5, 500)),
            self.stats(0.4, 50, {"1": 23, "2": 20}, busy, (10, 1500)),
        ])
        text = self.report("kudari")
        row = next(line.split() for line in text.split("what the build with the feature stats counted")[1].splitlines() if line.startswith("plain "))
        # 40 times woken, 20 refreshes and 40 paints in the 0.4 s between the lines; 95 % of 20
        # waits need the two in the bucket up to 1024 µs; ten frames built in 1500 µs.
        self.assertEqual(row[:5], ["plain", "100", "50", "100", "1024"])
        self.assertEqual(row[7], "150")

    def monotonic_run(self, name, log_lines, loads, trips=(), wall=None, keys=None):
        """A run as the driver writes it now: seconds on the monotonic clock, from 1000 on, and
        for each load the last byte written and whether the terminal answered."""
        lines = ["size 40 100"]
        if wall is not None:
            lines.append(f"clock 1000.000000 {wall:.6f}")
        lines.append("round-trips " + " ".join(f"{taken:.6f}" for taken in trips))
        for load, start, end, written, answered in loads:
            lines.append(f"{load} {1000 + start:.6f} {1000 + end:.6f} {1000 + written:.6f} {'answered' if answered else 'unanswered'}")
            if keys and load in keys:
                lines.append(f"keys {load} " + " ".join(f"{1000 + pressed:.6f}:{1000 + came:.6f}" for pressed, came in keys[load]))
        (self.directory / f"{name}.txt").write_text("\n".join(lines + ["done"]) + "\n")
        (self.directory / f"{name}.log").write_text("\n".join(log_lines) + "\n")

    def test_a_load_on_the_monotonic_clock_ends_with_the_answer_of_the_terminal(self):
        # Qt pads the seconds of %{time boot} with spaces; 12 frames up to 50 ms after the end.
        frame = "  {:.3f} qt.scenegraph.time.renderloop [render thread 0x1] syncAndRender: frame rendered in 3ms, sync=1, render=1, swap=1"
        self.monotonic_run("kudari-1", [frame.format(1000 + 0.02 * index) for index in range(12)], (("plain", 0.0, 0.2, 0.15, True),), trips=(0.001, 0.002, 0.003))
        measured = bench.measure(self.directory, "kudari-1")[1]["plain"]
        self.assertAlmostEqual(measured.seconds, 0.2)
        self.assertAlmostEqual(measured.backlog, 0.05)
        self.assertEqual(len(measured.frames), 12)
        text = self.report("kudari")
        self.assertEqual(self.frames_row(text, "plain")[1], "48.0")
        row = next(line.split() for line in text.split("A load ends when the terminal answers")[1].splitlines() if line.startswith("kudari "))
        # The query alone: 2 ms in the median, 3 ms at 95 %; plain: 50 ms after the last byte.
        self.assertEqual(row[:5], ["kudari", "2.00", "3.00", "50.0", "(50.0)"])

    def test_a_terminal_that_does_not_answer_ends_with_its_last_byte_and_is_named(self):
        self.monotonic_run("alacritty-1", [], (("plain", 0.0, 0.15, 0.15, False),))
        self.monotonic_run("alacritty-2", [], (("plain", 0.0, 0.25, 0.15, True),))
        row = next(line for line in self.report("alacritty").split("A load ends when")[1].splitlines() if line.startswith("alacritty "))
        self.assertIn("100.0 (100.0) 1 unanswered", row)

    def test_the_counters_on_the_time_of_day_match_the_loads_on_the_monotonic_clock(self):
        # The wall clock was 5000 s ahead of the monotonic one when the driver began.
        wall = 1000 + 5000
        line = lambda seconds, wakeups: self.stats(0, wakeups, {}, [], (0, 0)).replace(f"{bench.when(stamp(0)):.3f}", f"{wall + seconds:.3f}")
        self.monotonic_run("kudari-1", [line(-0.1, 10), line(0.3, 50)], (("plain", 0.0, 0.2, 0.2, True),), wall=wall)
        counted = bench.measure(self.directory, "kudari-1")[1]["plain"].counters
        self.assertEqual(counted["wakeups"], 40)
        self.assertAlmostEqual(counted["seconds"], 0.4)

    def test_typing_gives_the_time_from_each_key_to_the_terminal_and_to_its_frame(self):
        frame = "{:.3f} qt.scenegraph.time.renderloop [render thread 0x1] syncAndRender: frame rendered in 3ms, sync=1, render=1, swap=1"
        # Keys 0.1 s apart, each at the terminal 1 ms after its press and drawn 8 ms after it.
        keys = [(0.1 * index, 0.1 * index + 0.001) for index in range(100)]
        frames = [frame.format(1000 + pressed + 0.008) for pressed, _ in keys]
        self.monotonic_run("kudari-1", frames, (("typing", 0.0, 10.0, 10.0, True),), keys={"typing": keys})
        self.monotonic_run("alacritty-1", [], (("typing", 0.0, 10.0, 10.0, True),), keys={"typing": keys[:98]})
        text = self.report("kudari", "alacritty")
        part = text.split("typing: 100 keys")[1].splitlines()
        kudari = next(line.split() for line in part if line.startswith("kudari "))
        self.assertEqual(kudari[1:], ["1.00", "1.00", "1.00", "8.0", "8.0", "1.00", "0"])
        alacritty = next(line.split() for line in part if line.startswith("alacritty "))
        # Without frames in its log; two keys never came.
        self.assertEqual(alacritty[4:], ["-", "-", "-", "2"])
        # A load of keys has no throughput, but a cost per key.
        self.assertNotIn("\ntyping: 100 keys 50 ms apart, each echoed at once\n", text)

    def test_a_terminal_that_ended_during_a_load_is_named_with_it(self):
        self.monotonic_run("alacritty-1", [], (("plain", 0.0, 0.2, 0.2, True),))
        with open(self.directory / "alacritty-1.txt") as file:
            lines = file.read().splitlines()
        # It began titles after plain, and the bench found it gone.
        lines = lines[:-1] + ["begin titles", "ended"]
        (self.directory / "alacritty-1.txt").write_text("\n".join(lines) + "\n")
        text = self.report("alacritty")
        self.assertIn("alacritty in run 1, during titles", text)
        # What it did before still counts.
        self.assertIn("\nplain: 24 MB of plain text", text)

    def test_the_round_trips_of_each_query_alone_and_under_output(self):
        self.monotonic_run("kudari-1", [], (("query", 0.0, 2.0, 2.0, True),))
        with open(self.directory / "kudari-1.txt", "a") as file:
            for name, _, _ in bench.ASKING:
                alone = " ".join(["0.001000"] * 3)
                busy = "0.004000 0.004000 0.008000 -" if name == "background" else " ".join(["0.004000"] * 3)
                file.write(f"asked {name} alone {alone}\nasked {name} busy {busy}\n")
        part = self.report("kudari").split("query: 200 round trips")[1].splitlines()
        rows = [line.split() for line in part if line.startswith("kudari ")]
        self.assertEqual(rows[0][1:], ["cursor", "1.00", "1.00", "1.0", "0", "4.00", "4.00", "4.0", "0"])
        # The background under output: one of four got no answer.
        self.assertEqual(rows[2][1:], ["background", "1.00", "1.00", "1.0", "0", "4.00", "8.00", "8.0", "1"])

    def test_a_resize_gives_the_time_from_each_shortcut_to_its_frame(self):
        frame = "{:.3f} qt.scenegraph.time.renderloop [render thread 0x1] syncAndRender: frame rendered in 3ms, sync=1, render=1, swap=1"
        # 20 shortcuts 0.5 s apart, each drawn 30 ms after it; the frames 470 ms apart between.
        presses = [0.5 * index for index in range(20)]
        frames = [frame.format(1000 + pressed + 0.03) for pressed in presses]
        self.monotonic_run("kudari-1", frames, (("resize", 0.0, 10.0, 10.0, True),), keys={"resize": [(pressed, pressed) for pressed in presses]})
        part = self.report("kudari").split("resize: 10 steps")[1].splitlines()
        row = next(line.split() for line in part if line.startswith("kudari "))
        self.assertEqual(row[1:], ["20", "30.0", "30.0", "30.0", "500"])

    def test_older_runs_on_the_time_of_day_still_read(self):
        self.run_of("kudari-1", [threaded(0.1, 1)])
        measured = bench.measure(self.directory, "kudari-1")[1]["plain"]
        self.assertIsNone(measured.backlog)
        self.assertNotIn("A load ends when", self.report("kudari"))

    def test_a_load_of_a_fixed_time_costs_per_change_and_has_no_throughput(self):
        self.run_of("kudari-1", [], (("partial", 0.0, 10.0),))
        self.cpu_of("kudari-1", "partial", (1, "main", 0, 625), (2, "QSGRenderThread", 0, 625))
        text = self.report("kudari")
        self.assertNotIn("\npartial: a line of status", text)
        part = text.split("partial: CPU time of the terminal")[1].splitlines()
        self.assertIn("ms per change", part[1])
        row = next(line.split() for line in part if line.startswith("kudari "))
        # 1250 ms of CPU for 625 changes.
        self.assertEqual(row[1:4], ["1250", "12.5", "2.00"])

    def test_without_lines_around_a_load_the_counters_say_nothing(self):
        self.run_of("kudari-1", [self.stats(0.1, 10, {}, [], (0, 0)), self.stats(0.4, 20, {}, [], (0, 0))])
        self.assertNotIn("feature stats", self.report("kudari"))
        self.run_of("kudari-1", [threaded(0.1, 1)])
        self.assertNotIn("feature stats", self.report("kudari"))

    def test_the_buckets_give_the_upper_end_of_the_share(self):
        # Two of four lie in the first two buckets, the second up to 2 µs.
        self.assertEqual(bench.bucket_quantile([1, 1, 2], 0.5), 2)
        self.assertEqual(bench.bucket_quantile([3, 0, 0, 1], 0.95), 8)
        self.assertIsNone(bench.bucket_quantile([0, 0], 0.5))

    def test_a_profiled_run_names_the_command_for_each_load(self):
        self.run_of("kudari-1", [threaded(0.1, 1)])
        (self.directory / "kudari-1.perf").write_text("plain 100.000001 100.200002\n")
        (self.directory / "perf-kudari-1.data").write_text("")
        # A warm-up run has no command.
        (self.directory / "kudari-w1.perf").write_text("plain 90.0 90.2\n")
        (self.directory / "perf-kudari-w1.data").write_text("")
        text = self.report("kudari")
        self.assertIn(f"kudari run 1, plain: perf report -i {self.directory / 'perf-kudari-1.data'} --time 100.000001,100.200002 --sort comm,dso,sym", text)
        self.assertNotIn("run w1", text)
        self.assertNotIn("Profiles of perf", self.report_without_perf())

    def report_without_perf(self):
        for path in self.directory.glob("*perf*"):
            path.unlink()
        return self.report("kudari")

    def comparison(self, text, title, build):
        part = text.split("The builds of Kudari against")[1].split(title)[1]
        return next(line.split() for line in part.splitlines() if line.startswith("plain ") and build in line)

    def test_builds_compare_with_the_first_and_get_a_verdict(self):
        # Five runs each: b takes half again as long and as much CPU time, c the same as a.
        for run in range(1, 6):
            wobble = 0.002 * run
            for build, seconds, cpu in (("a", 0.20, 100), ("b", 0.30, 150), ("c", 0.20, 100)):
                self.run_of(f"kudari@{build}-{run}", [], (("plain", 0.0, seconds + wobble),))
                self.cpu_of(f"kudari@{build}-{run}", "plain", (1, "PTY reader", 0, cpu + run))
        text = self.report("kudari@a", "kudari@b", "kudari@c")
        # Medians of 0.206 and 0.306 s.
        self.assertEqual(self.comparison(text, "seconds per load", "kudari@b")[1:7], ["kudari@b", "0.21", "0.31", "1.485", "1.452-1.520", "slower"])
        self.assertEqual(self.comparison(text, "seconds per load", "kudari@c")[-2:], ["no", "difference"])
        self.assertEqual(self.comparison(text, "CPU ms per load", "kudari@b")[-1], "more")
        # Nothing to compare with one build.
        for path in [*self.directory.glob("kudari@b-*"), *self.directory.glob("kudari@c-*")]:
            path.unlink()
        self.assertNotIn("The builds of Kudari against", self.report("kudari@a"))

    def test_a_verdict_needs_an_interval_without_one(self):
        self.assertEqual(bench.verdict(0.90, 0.99), "faster")
        self.assertEqual(bench.verdict(1.01, 1.10), "slower")
        self.assertEqual(bench.verdict(0.95, 1.02), "no difference")
        self.assertEqual(bench.verdict(0.5, 0.6, ("less", "more")), "less")

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


class Scenarios(unittest.TestCase):
    def setUp(self):
        self.directory = Path(tempfile.mkdtemp())
        self.addCleanup(lambda: [path.unlink() for path in self.directory.iterdir()] and self.directory.rmdir())

    def at(self, seconds):
        return bench.when(stamp(seconds))

    def write(self, name, lines):
        (self.directory / name).write_text("".join(line + "\n" for line in lines))

    def cpu(self, scenario, start_ns, end_ns, switches=(0, 0), memory=None):
        lines = [f"{scenario}\tstart\t1\tmain\t{start_ns}\t0\t0", f"{scenario}\tend\t1\tmain\t{end_ns}\t{switches[0]}\t{switches[1]}"]
        if memory:
            lines.append(f"{scenario}\tmemory\t{memory[0]}\t{memory[1]}\t{memory[2]}")
        return lines

    def report(self):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            bench.scenarios(self.directory)
        return output.getvalue()

    def row(self, text, label):
        return next(line.split() for line in text.splitlines() if line.startswith(label))

    def test_each_scenario_is_timed_from_its_events(self):
        self.write("events-1.txt", [
            f"spawn main {self.at(1.0)}", f"bus main {self.at(1.05)}",
            f"show hidden {self.at(5.0)}", f"show main {self.at(10.0)}", f"show main {self.at(12.0)}",
            f"second-start main {self.at(20.0)}", f"second-end main {self.at(20.04)}",
            f"idle shown {self.at(30.0)}", f"idle-end shown {self.at(40.0)}",
        ])
        self.write("main-1.log", [
            threaded(1.14, 1), f"kudari-timing qml {self.at(1.12)}", f"kudari-timing toggle {self.at(10.003)}",
            # 6 ms after the first showing, 8 ms after the second; a frame 3 s later is no answer.
            threaded(10.006, 1), threaded(12.008, 1), threaded(16.0, 1),
            # Five frames in the ten seconds of waiting.
            *(threaded(31.0 + index, 1) for index in range(5)),
        ])
        self.write("hidden-1.log", [threaded(5.01, 1)])
        # 2 ms of CPU and 30 switches in 10 s; 100 MB private after the first tab.
        self.write("main-1.cpu", self.cpu("shown", 0, 2_000_000, (20, 10)) + self.cpu("memory-1", 0, 0, memory=(300 * 1024, 110 * 1024, 100 * 1024)))
        # A warm-up run does not count.
        self.write("events-w1.txt", [f"spawn main {self.at(0.0)}", f"bus main {self.at(9.0)}"])
        text = self.report()
        self.assertIn("1 runs of the scenarios", text)
        self.assertEqual(self.row(text, "name on the bus")[4:6], ["50.0", "50.0"])
        self.assertEqual(self.row(text, "first frame")[2], "140.0")
        self.assertEqual(self.row(text, "phase qml")[2], "120.0")
        self.assertEqual(self.row(text, "first time")[2], "10.0")
        self.assertEqual(self.row(text, "the times after")[3:7], ["7.0", "8.0", "8.0", "2"])
        self.assertEqual(self.row(text, "call received")[2], "3.0")
        self.assertEqual(self.row(text, "second start")[2], "40.0")
        self.assertEqual(self.row(text, "shown, nothing happens")[3:6], ["0.20", "3", "0.5"])
        # The busiest thread, in milliseconds a second.
        self.assertEqual(self.row(text, "shown, nothing happens")[6:], ["main", "0.20"])
        self.assertEqual(self.row(text, "1 tab")[2:5], ["100", "300", "110"])

    def test_without_a_run_it_says_so(self):
        self.write("events-w1.txt", [])
        self.assertIn("No run of the scenarios finished", self.report())


class Statistics(unittest.TestCase):
    def test_close_runs_are_no_outliers_for_the_clock_alone(self):
        self.assertEqual(bench.outliers([("1", 0.180), ("2", 0.182), ("3", 0.181)]), [])

    def test_equal_runs_have_no_outliers(self):
        self.assertEqual(bench.outliers([("1", 1.0), ("2", 1.0), ("3", 1.0)]), [])

    def test_the_interval_of_one_run_is_that_run(self):
        self.assertEqual(bench.interval([0.5]), (0.5, 0.5))

    def test_the_interval_holds_the_median(self):
        low, high = bench.interval([1.0, 1.1, 1.2, 1.3, 5.0])
        self.assertLessEqual(low, 1.2)
        self.assertGreaterEqual(high, 1.2)

    def test_the_order_is_shuffled_for_each_run_and_repeats_with_its_seed(self):
        terminals = ["kudari", "konsole", "alacritty", "kitty"]
        orders = [bench.order("7", str(run), terminals) for run in range(1, 21)]
        self.assertTrue(all(sorted(found) == sorted(terminals) for found in orders))
        self.assertGreater(len({tuple(found) for found in orders}), 1)
        self.assertEqual(orders, [bench.order("7", str(run), terminals) for run in range(1, 21)])


class Environment(unittest.TestCase):
    def describe(self, kudari=None, removed=(), kept=False, **machine):
        return bench.describe(kudari, {**MACHINE, **machine}, SOFTWARE, list(removed), kept)

    def warnings(self, *arguments, **machine):
        return [line for line in self.describe(*arguments, **machine) if line.startswith("Warning:")]

    def test_builds_to_compare_are_each_named(self):
        lines = self.describe([("kudari@main", KUDARI), ("kudari@new", {**KUDARI, "path": "/tmp/new/kudari"})])
        self.assertTrue(any(line.startswith("kudari@main ") and KUDARI["path"] in line for line in lines))
        self.assertTrue(any(line.startswith("kudari@new ") and "/tmp/new/kudari" in line for line in lines))

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

    def test_the_settings_for_kudari_are_named(self):
        lines = bench.describe(None, MACHINE, SOFTWARE, [], False, bench.Plan(5, 0, "1", ("Terminal.BlinkingCursor=true",)))
        self.assertEqual(lines[1], "Settings  Terminal.BlinkingCursor=true for Kudari")

    def test_the_plan_names_runs_warm_up_and_seed(self):
        lines = bench.describe(None, MACHINE, SOFTWARE, [], False, bench.Plan(5, 1, "42"))
        self.assertEqual(lines[0], "Runs      5 after 1 warm-up run that does not count; the terminals in an order shuffled for each run, --seed 42 repeats it")

    def test_the_variables_say_whether_they_were_removed(self):
        self.assertIn("QT_SCALE_FACTOR, LD_PRELOAD: removed", "\n".join(self.describe(removed=["QT_SCALE_FACTOR", "LD_PRELOAD"])))
        self.assertIn("kept, with --keep-env", "\n".join(self.describe(removed=["QT_SCALE_FACTOR"], kept=True)))


compare_spec = importlib.util.spec_from_file_location("bench_compare", TOOLS / "bench-compare.py")
compare = importlib.util.module_from_spec(compare_spec)
compare_spec.loader.exec_module(compare)


class Compare(unittest.TestCase):
    def test_the_figures_come_from_the_csv_of_the_benchmark(self):
        csv = "frame,text,us,270.5\nframe,text,instructions,8916201\nnot a figure\ncases,keys,us,oops\n"
        self.assertEqual(compare.figures(csv), {("frame", "text", "us"): 270.5, ("frame", "text", "instructions"): 8916201.0})

    def test_the_rounds_give_their_median(self):
        rounds = [{("frame", "text", "us"): value} for value in (300.0, 270.0, 280.0)]
        self.assertEqual(compare.combine(rounds), {("frame", "text", "us"): 280.0})

    def marks(self, base, head):
        # The metric from its column: names such as "link at" have spaces.
        return {line[31:52].strip(): line.endswith("*") for line in compare.report(base, head)[2:] if line.strip() and not line.startswith("*")}

    def test_a_change_of_instructions_is_marked_and_the_noise_of_times_is_not(self):
        base = {("frame", "text", "us"): 270.0, ("frame", "text", "instructions"): 8_900_000.0}
        # Three percent more instructions, and a time that moved by ten percent as well.
        head = {("frame", "text", "us"): 297.0, ("frame", "text", "instructions"): 9_167_000.0}
        self.assertEqual(self.marks(base, head), {"us": False, "instructions": True})
        # The same instructions: a time that moved is noise.
        head[("frame", "text", "instructions")] = 8_900_100.0
        self.assertEqual(self.marks(base, head), {"us": False, "instructions": False})
        # More than the floor, but a tenth of a percent of a large frame: noise as well.
        head[("frame", "text", "instructions")] = 8_910_000.0
        self.assertEqual(self.marks(base, head), {"us": False, "instructions": False})

    def test_a_small_case_needs_more_than_the_floor_to_count(self):
        base = {("cases", "link at", "instructions"): 19_729.0}
        self.assertEqual(self.marks(base, {("cases", "link at", "instructions"): 19_888.0}), {"instructions": False})
        self.assertEqual(self.marks(base, {("cases", "link at", "instructions"): 21_000.0}), {"instructions": True})

    def test_without_counts_the_times_decide(self):
        base = {("throughput", "plain", "parser_ms"): 130.0, ("throughput", "plain", "parser_instructions"): 0.0}
        self.assertEqual(self.marks(base, {("throughput", "plain", "parser_ms"): 140.0, ("throughput", "plain", "parser_instructions"): 0.0}), {"parser_ms": True})
        self.assertEqual(self.marks(base, {("throughput", "plain", "parser_ms"): 133.0, ("throughput", "plain", "parser_instructions"): 0.0}), {"parser_ms": False})


class Loads(unittest.TestCase):
    """The data of the loads, from a fixed seed."""

    def setUp(self):
        self.directory = Path(tempfile.mkdtemp())
        self.addCleanup(lambda: [path.unlink() for path in self.directory.iterdir()] and self.directory.rmdir())

    def screens(self, text):
        return [screen for screen in text.split("\x1b[H") if screen]

    def test_the_panels_fill_each_screen_with_lines_blocks_and_braille(self):
        import re as expressions

        bench.boxes(self.directory, 12, 40)
        text = (self.directory / "boxes-12x40.txt").read_bytes().decode()
        screens = self.screens(text)
        self.assertEqual(len(screens), 300)
        for screen in screens[:20]:
            rows = expressions.sub(r"\x1b\[[0-9;]*m", "", screen).split("\r\n")
            # Every row is as wide as the terminal: nothing wraps or scrolls.
            self.assertEqual([len(row) for row in rows], [40] * 12)
        self.assertTrue(any(kind[0] in text for kind in bench.LINES))
        self.assertTrue(any(chr(0x2800) <= character <= chr(0x28FF) for character in text))
        self.assertIn(bench.BLOCKS[7], text)
        # The same again from the seed.
        first = text
        (self.directory / "boxes-12x40.txt").unlink()
        bench.boxes(self.directory, 12, 40)
        self.assertEqual((self.directory / "boxes-12x40.txt").read_bytes().decode(), first)

    def test_the_unicode_text_holds_wide_characters_emoji_and_marks(self):
        bench.data(self.directory)
        text = (self.directory / "unicode.txt").read_text()
        self.assertEqual(text.count("\n"), 150_000)
        for piece in ("日本語", "👩‍💻", "👍🏽", "e\u0301"):
            self.assertIn(piece, text)


class Driver(unittest.TestCase):
    """The parts of the driver of the loads that need no terminal under test."""

    def setUp(self):
        import pty
        import termios
        import threading

        self.master, self.slave = pty.openpty()
        self.addCleanup(os.close, self.master)
        self.addCleanup(os.close, self.slave)
        mode = termios.tcgetattr(self.slave)
        mode[3] &= ~(termios.ICANON | termios.ECHO)
        termios.tcsetattr(self.slave, termios.TCSANOW, mode)
        self.threading = threading

    def terminal(self, reply, after=0.0):
        """Reads the query on the side of the terminal and answers it with `reply`, `after`
        seconds later."""

        def serve():
            seen = b""
            while b"\x1b[6n" not in seen:
                seen += os.read(self.master, 64)
            time.sleep(after)
            if reply:
                os.write(self.master, reply)

        thread = self.threading.Thread(target=serve, daemon=True)
        thread.start()
        return thread

    def test_the_answer_to_a_query_of_the_cursor_ends_the_wait(self):
        # An answer that came too late for a query before does not count for this one.
        os.write(self.master, b"\x1b[1;1R")
        time.sleep(0.1)
        self.terminal(b"\x1b[12;40R", after=0.3)
        before = bench.monotonic()
        answered = bench.answer(self.slave, self.slave, wait=5)
        self.assertIsNotNone(answered)
        self.assertGreaterEqual(answered - before, 0.25)
        self.assertLess(answered - before, 5)

    def test_without_an_answer_the_wait_ends_after_its_time(self):
        self.terminal(b"")
        self.assertIsNone(bench.answer(self.slave, self.slave, wait=0.2))

    def test_a_line_of_status_changes_on_time_and_leaves_the_rest(self):
        reader, writer = os.pipe()
        self.addCleanup(os.close, reader)
        changes = bench.partial(writer, 60, seconds=0.2, every=0.02)
        os.close(writer)
        written = os.read(reader, 1 << 20).decode()
        # Ten changes in 0.2 s, each saving the cursor, writing the top line and going back.
        self.assertIn(changes, range(9, 12))
        self.assertEqual(written.count("\x1b7\x1b[1;1H"), changes)
        self.assertEqual(written.count("\x1b8"), changes)

    def test_each_key_that_comes_is_echoed_and_paired_with_its_press(self):
        import sys as system

        def keys():
            for _ in range(3):
                time.sleep(0.05)
                os.write(self.master, b"a")

        self.threading.Thread(target=keys, daemon=True).start()
        presses = "import time\nfor _ in range(3):\n    print(time.clock_gettime(time.CLOCK_MONOTONIC), flush=True)\n    time.sleep(0.05)\n"
        paired = bench.typed(self.slave, self.slave, [system.executable, "-c", presses], 3)
        self.assertEqual(len(paired), 3)
        self.assertTrue(all(pressed < bench.monotonic() and came <= bench.monotonic() for pressed, came in paired))
        # The echo goes back to the terminal.
        self.assertEqual(os.read(self.master, 64), b"aaa")

    def test_queries_go_out_in_batches_each_answered_before_the_next(self):
        source = Path(tempfile.mkdtemp()) / "queries.txt"
        self.addCleanup(lambda: (source.unlink(), source.parent.rmdir()))
        source.write_bytes(b"\x1b[c" * 2500)
        seen = []

        def terminal(limit):
            asked = 0
            while asked < 2500:
                got = os.read(self.master, 65536).count(b"\x1b[c")
                seen.append(got)
                # A terminal that answers no more than `limit` of all questions.
                for _ in range(max(0, min(got, limit - asked))):
                    os.write(self.master, b"\x1b[?62;22c")
                asked += got
                if asked >= limit:
                    return

        self.threading.Thread(target=terminal, args=(2500,), daemon=True).start()
        self.assertEqual(bench.asked(self.slave, self.slave, source, 2500, batch=1000), 2500)
        # No more than a batch went out before its answers came.
        self.assertLessEqual(max(seen), 1000)

    def test_a_batch_without_its_answers_ends_the_queries(self):
        source = Path(tempfile.mkdtemp()) / "queries.txt"
        self.addCleanup(lambda: (source.unlink(), source.parent.rmdir()))
        source.write_bytes(b"\x1b[c" * 300)

        def terminal():
            asked = 0
            while asked < 100:
                asked += os.read(self.master, 65536).count(b"\x1b[c")
            # Only the first 100 get an answer.
            os.write(self.master, b"\x1b[?62;22c" * 100)

        self.threading.Thread(target=terminal, daemon=True).start()
        original, bench.ANSWER = bench.ANSWER, 0.3
        try:
            self.assertEqual(bench.asked(self.slave, self.slave, source, 300, batch=100), 100)
        finally:
            bench.ANSWER = original

    def answering(self, background):
        """A terminal that answers the cursor and the device attributes, and the background
        only if `background`."""

        def serve():
            seen = b""
            while True:
                try:
                    seen += os.read(self.master, 256)
                except OSError:
                    return
                while True:
                    if seen.startswith(b"\x1b[6n"):
                        os.write(self.master, b"\x1b[3;4R")
                        seen = seen[4:]
                    elif seen.startswith(b"\x1b[c"):
                        os.write(self.master, b"\x1b[?62;22c")
                        seen = seen[3:]
                    elif seen.startswith(b"\x1b]11;?\x07"):
                        if background:
                            os.write(self.master, b"\x1b]11;rgb:1e1e/1e1e/2e2e\x1b\\")
                        seen = seen[len(b"\x1b]11;?\x07") :]
                    else:
                        break

        self.threading.Thread(target=serve, daemon=True).start()

    def test_a_round_trip_ends_with_its_answer(self):
        self.answering(background=True)
        for _, query, pattern in bench.ASKING:
            taken = bench.round_trip(self.slave, self.slave, query, pattern)
            self.assertIsNotNone(taken, query)
            self.assertLess(taken, 1.0)

    def test_a_query_the_terminal_does_not_answer_ends_with_the_one_behind_it(self):
        self.answering(background=False)
        _, query, pattern = bench.ASKING[2]
        before = bench.monotonic()
        self.assertIsNone(bench.round_trip(self.slave, self.slave, query, pattern, wait=5.0))
        # Not after the wait, but as soon as the answer to the device attributes came.
        self.assertLess(bench.monotonic() - before, 1.0)

    def test_a_file_goes_out_whole_in_blocks(self):
        source = Path(tempfile.mkdtemp()) / "load.txt"
        self.addCleanup(lambda: (source.unlink(), source.parent.rmdir()))
        source.write_bytes(bytes(range(256)) * (bench.BLOCK // 128 + 3))
        reader, writer = os.pipe()
        got = bytearray()

        def drain():
            while chunk := os.read(reader, 65536):
                got.extend(chunk)

        thread = self.threading.Thread(target=drain)
        thread.start()
        bench.write_all(writer, source)
        os.close(writer)
        thread.join()
        os.close(reader)
        self.assertEqual(bytes(got), source.read_bytes())


if __name__ == "__main__":
    sys.exit(unittest.main())
