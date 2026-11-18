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
        rss, pss, private = map(int, memory[0][2:])
        self.assertGreater(rss, 0)
        # Python shares its libraries with other processes, so not all of it is private.
        self.assertLess(private, rss)

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
        self.assertEqual(row, ["kudari", "95", "96", "310", "120"])
        self.assertIn("after plain  after idle", text)

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


if __name__ == "__main__":
    sys.exit(unittest.main())
