#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
"""Compares the core benchmark of two states of Kudari: what a change costs or gains.

    tools/bench-compare.py [BASE] [--rounds N] [--runs N] [PART...]

BASE is a commit, the merge base of HEAD with origin/main by default; the other state is
the working tree as it is, with what is not committed yet. PART picks some of throughput,
frame and cases, all by default. Each state builds `cargo bench -p kudari-core --bench core`,
BASE in a worktree of its own under ~/.cache, and the two run in turn, ROUNDS times each (3
by default), so that a change of the clock meanwhile hits both alike.

The report gives for each figure the median of the rounds of both and the change. The
instructions vary by far less than a percent from run to run, the times by a few: a change
of instructions of more than 0.5 % and 1000 instructions is marked, one of times only where
nothing was counted and the times moved by more than 5 %. The counts need Linux and perf_event_paranoid of 2 or
less; without them the report has times only.
"""

import argparse
import json
import os
import statistics
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CACHE = Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache")) / "kudari-bench-compare"
# Changes beyond these count as such; the instructions are near exact, the times are not. A
# small case of a few thousand instructions varies by some hundred with the state of the
# allocator, so a change of instructions must also be more than INSTRUCTIONS_FLOOR.
INSTRUCTIONS_NOISE = 0.005
INSTRUCTIONS_FLOOR = 1000
TIME_NOISE = 0.05


def run(*command, cwd=ROOT, **options):
    return subprocess.run(command, cwd=cwd, check=True, text=True, capture_output=True, **options).stdout


def build(directory, target):
    """Builds the benchmark in `directory` and gives the path of its program."""
    output = run(
        "cargo", "bench", "--locked", "-q", "-p", "kudari-core", "--bench", "core", "--no-run",
        "--message-format=json", cwd=directory, env={**os.environ, "CARGO_TARGET_DIR": str(target)},
    )
    for line in output.splitlines():
        message = json.loads(line)
        if message.get("reason") == "compiler-artifact" and message.get("target", {}).get("name") == "core" and message.get("executable"):
            return message["executable"]
    sys.exit(f"bench-compare: cargo built no benchmark in {directory}")


def figures(csv):
    """The figures of one run, from the CSV of the benchmark: (part, name, metric) to value."""
    found = {}
    for line in csv.splitlines():
        parts = line.split(",")
        if len(parts) == 4:
            try:
                found[tuple(parts[:3])] = float(parts[3])
            except ValueError:
                continue
    return found


def combine(rounds):
    """The median of each figure over the rounds."""
    keys = []
    for figures_of_round in rounds:
        keys.extend(key for key in figures_of_round if key not in keys)
    return {key: statistics.median(round_[key] for round_ in rounds if key in round_) for key in keys}


def change(base, head):
    return (head - base) / base if base else 0.0


def report(base, head):
    """The lines of the comparison. The instructions of a figure decide whether it changed;
    its time counts only where both have no instructions to go by."""
    lines = []
    names = []
    for part, name, metric in base:
        if (part, name) not in names:
            names.append((part, name))
    header = f"{'part':<11} {'name':<18} {'metric':<21} {'base':>14} {'head':>14} {'change':>8}"
    lines.append(header)
    lines.append("-" * len(header))
    for part, name in names:
        metrics = [metric for (p, n, metric) in base if (p, n) == (part, name)]
        counted = {metric.removesuffix("instructions").rstrip("_") for metric in metrics if metric.endswith("instructions") and base.get((part, name, metric))}
        for metric in metrics:
            key = (part, name, metric)
            if key not in head:
                continue
            delta = change(base[key], head[key])
            if metric.endswith("instructions"):
                if not base[key]:
                    continue
                moved = abs(head[key] - base[key]) > INSTRUCTIONS_FLOOR
                mark = " *" if abs(delta) > INSTRUCTIONS_NOISE and moved else ""
                shown = (f"{base[key]:>14.0f}", f"{head[key]:>14.0f}")
            else:
                stem = metric.removesuffix("_ms").removesuffix("us").rstrip("_")
                mark = " *" if abs(delta) > TIME_NOISE and stem not in counted else ""
                shown = (f"{base[key]:>14.2f}", f"{head[key]:>14.2f}")
            lines.append(f"{part:<11} {name:<18} {metric:<21} {shown[0]} {shown[1]} {delta:>+8.1%}{mark}")
    lines.append("")
    lines.append(
        f"* a change of instructions beyond {INSTRUCTIONS_NOISE:.1%} and {INSTRUCTIONS_FLOOR}, "
        f"or of a time beyond {TIME_NOISE:.0%} where nothing was counted"
    )
    return lines


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("base", nargs="?", help="the commit to compare with; the merge base with origin/main by default")
    parser.add_argument("parts", nargs="*", default=[], help="throughput, frame or cases; all by default")
    parser.add_argument("--rounds", type=int, default=3, help="times each state runs, in turn")
    parser.add_argument("--runs", type=int, default=3, help="runs of each figure within one round")
    arguments = parser.parse_args()
    if arguments.base in ("throughput", "frame", "cases"):
        arguments.parts.insert(0, arguments.base)
        arguments.base = None
    base = arguments.base or run("git", "merge-base", "HEAD", "origin/main").strip()
    base = run("git", "rev-parse", "--short", base).strip()
    worktree = CACHE / "worktree"
    if worktree.exists():
        run("git", "worktree", "remove", "--force", str(worktree))
    CACHE.mkdir(parents=True, exist_ok=True)
    run("git", "worktree", "add", "--detach", str(worktree), base)
    try:
        print(f"Building {base} and the working tree ...", file=sys.stderr)
        programs = {
            "base": build(worktree, CACHE / "target"),
            "head": build(ROOT, Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))),
        }
        rounds = {"base": [], "head": []}
        for number in range(arguments.rounds):
            for state in ("base", "head"):
                print(f"Round {number + 1} of {arguments.rounds}: {state}", file=sys.stderr)
                csv = run(programs[state], *arguments.parts, "--csv", "--runs", str(arguments.runs))
                rounds[state].append(figures(csv))
    finally:
        run("git", "worktree", "remove", "--force", str(worktree))
    print(f"base {base} against the working tree, median of {arguments.rounds} rounds")
    print()
    for line in report(combine(rounds["base"]), combine(rounds["head"])):
        print(line)


if __name__ == "__main__":
    main()
