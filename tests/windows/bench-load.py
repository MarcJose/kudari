# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
"""The shell of Kudari while tests/windows/bench.ps1 measures it: writes each load to the
terminal and times it, and writes a line for each to the file out.txt of RUN. The loads are
those of tools/bench.py, from its data in DATA. It tells that it is ready with the file ready
in RUN, and starts once the file go is there.

    bench-load.py DATA RUN LOAD...

Under ConPTY the console host answers queries such as that of the cursor itself, from its own
copy of the screen, so their answer tells nothing about Kudari. A copy to the clipboard with
OSC 52 goes on to Kudari, which acts on it once it has read all that came before. So a load
ends when the clipboard changes after it, which its sequence number tells without opening it.
Kudari and its console host are the parent of this program and a child of that parent."""

import base64
import ctypes
import importlib.util
import os
import sys
import time
from ctypes import wintypes
from pathlib import Path

spec = importlib.util.spec_from_file_location("bench", Path(__file__).resolve().parent.parent.parent / "tools" / "bench.py")
bench = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bench)

kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
user32 = ctypes.WinDLL("user32", use_last_error=True)
ENABLE_PROCESSED_INPUT, ENABLE_LINE_INPUT, ENABLE_ECHO_INPUT = 0x1, 0x2, 0x4
ENABLE_VIRTUAL_TERMINAL_INPUT = 0x200
ENABLE_VIRTUAL_TERMINAL_PROCESSING = 0x4
PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
TH32CS_SNAPPROCESS = 0x2
HOSTS = ("conhost.exe", "openconsole.exe")
# How long a load may take before it counts as unanswered.
WAIT = 120.0
CHUNK = 1 << 20


class ProcessEntry(ctypes.Structure):
    _fields_ = [
        ("dwSize", wintypes.DWORD), ("cntUsage", wintypes.DWORD), ("th32ProcessID", wintypes.DWORD),
        ("th32DefaultHeapID", ctypes.c_size_t), ("th32ModuleID", wintypes.DWORD), ("cntThreads", wintypes.DWORD),
        ("th32ParentProcessID", wintypes.DWORD), ("pcPriClassBase", ctypes.c_long), ("dwFlags", wintypes.DWORD),
        ("szExeFile", ctypes.c_wchar * 260),
    ]


kernel32.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
kernel32.OpenProcess.restype = wintypes.HANDLE


def children(parent):
    """The processes whose parent is `parent`, as pairs of their id and their name."""
    snapshot = kernel32.CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
    entry = ProcessEntry()
    entry.dwSize = ctypes.sizeof(entry)
    found = []
    more = kernel32.Process32FirstW(snapshot, ctypes.byref(entry))
    while more:
        if entry.th32ParentProcessID == parent:
            found.append((entry.th32ProcessID, entry.szExeFile.lower()))
        more = kernel32.Process32NextW(snapshot, ctypes.byref(entry))
    kernel32.CloseHandle(snapshot)
    return found


def opened(pid):
    return kernel32.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, False, pid) or None


def cpu_ms(handle):
    """The CPU time of a process so far, user and kernel, in milliseconds."""
    if handle is None:
        return 0.0
    times = [wintypes.FILETIME() for _ in range(4)]
    if not kernel32.GetProcessTimes(handle, *(ctypes.byref(each) for each in times)):
        return 0.0
    return sum((each.dwHighDateTime << 32 | each.dwLowDateTime) for each in times[2:]) / 10_000


def console_modes():
    """Input without echo and lines, and output whose sequences the console host acts on."""
    mode = wintypes.DWORD()
    reader, writer = kernel32.GetStdHandle(-10), kernel32.GetStdHandle(-11)
    kernel32.GetConsoleMode(reader, ctypes.byref(mode))
    kernel32.SetConsoleMode(reader, (mode.value & ~(ENABLE_PROCESSED_INPUT | ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT)) | ENABLE_VIRTUAL_TERMINAL_INPUT)
    kernel32.GetConsoleMode(writer, ctypes.byref(mode))
    kernel32.SetConsoleMode(writer, mode.value | ENABLE_VIRTUAL_TERMINAL_PROCESSING)


def write_all(data):
    # Python writes at most 32 KB to a console at a time.
    view = memoryview(data)
    while view:
        view = view[os.write(1, view[:CHUNK]):]


def numbers():
    """The output of seq 1 2000000."""
    return "".join(f"{number}\n" for number in range(1, 2_000_001)).encode()


def copied(before, until):
    """When the clipboard changed from `before`, or None if not before `until`."""
    while time.perf_counter() < until:
        if user32.GetClipboardSequenceNumber() != before:
            return time.perf_counter()
        time.sleep(0.001)
    return None


def main():
    directory, run, loads = Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3:]
    out = run / "out.txt"
    console_modes()
    terminal = os.getppid()
    hosts = [pid for pid, name in children(terminal) if name in HOSTS]
    handles = [opened(terminal), opened(hosts[0]) if len(hosts) == 1 else None]

    def note(line):
        with open(out, "a", encoding="utf-8") as file:
            file.write(line + "\n")

    columns, rows = os.get_terminal_size(1)
    bench.frames(directory, rows, columns)
    bench.boxes(directory, rows, columns)
    note(f"size {rows} {columns}")
    note(f"host {'found' if handles[1] else 'unknown'}")
    (run / "ready").touch()
    while not (run / "go").exists():
        time.sleep(0.1)
    for number, load in enumerate(loads):
        data = numbers() if load == "seq" else (directory / (f"{load}-{rows}x{columns}.txt" if load in ("frames", "boxes") else f"{load}.txt")).read_bytes()
        write_all(b"\x1b[2J\x1b[H")
        time.sleep(1)
        mark = base64.b64encode(f"kudari bench {load} {number}".encode())
        before = user32.GetClipboardSequenceNumber()
        used = [cpu_ms(handle) for handle in handles]
        clock, start = time.time(), time.perf_counter()
        write_all(data)
        written = time.perf_counter()
        write_all(b"\x1b]52;c;" + mark + b"\x07")
        end = copied(before, start + WAIT)
        used = [cpu_ms(handle) - was for handle, was in zip(handles, used)]
        answered = end is not None
        end = end if answered else written
        note(
            f"{load} {len(data)} {end - start:.6f} {written - start:.6f} {'answered' if answered else 'unanswered'}"
            f" {used[0]:.1f} {used[1]:.1f} {clock:.3f} {clock + end - start:.3f}"
        )
        time.sleep(1)
    note("done")
    while True:
        time.sleep(100_000)


if __name__ == "__main__":
    main()
