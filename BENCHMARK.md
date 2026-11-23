# Benchmark

How fast Kudari takes in output, how much processor time and memory it takes, how quickly
it answers a key and how still it stays while it waits, next to other terminals on the
same computer. `tools/bench.sh` makes all the numbers below; the last section says how to
repeat them. Compare numbers only from the same computer: the processor, the graphics card
and the versions of Qt, KWin and Mesa all change them.

## Setup

Measured on 2026-11-23 with Kudari 0.5.0, a release build of the tag.

| | |
|---|---|
| Computer | AMD Ryzen 7 9800X3D, 16 threads, governor `performance`; AMD Radeon RX 6600 |
| System | Arch Linux, Linux 7.3.5, KWin 6.8.3, Qt 6.12.0, Mesa 26.3.1 |
| Screen | a virtual KWin of 2560×1440 pixels at 60 Hz, scale 1; xterm through Xwayland |
| Terminals | Konsole and Yakuake 26.08.3, Alacritty 0.17.0, kitty 0.51.0, Ghostty 1.4.0, Contour 0.7.0, xterm |
| Font and history | Hack at 11 points and 10,000 lines of history for all, except Contour, which takes neither, and Ghostty, which counts its history in bytes |
| Runs | 5 for each terminal and load, after a warm-up run that does not count, in an order shuffled for each run |

Each terminal runs full screen in the virtual KWin, on a D-Bus of its own and with settings
of its own. A load counts from its first byte until the terminal answers a query of the
cursor after its last one, when it has read and parsed all of it. Drawing goes on at the
rate of the screen meanwhile. With the same font, the terminals still have different
numbers of cells, from 280×76 to 284×84 (Contour 256×62), as the height of a line, the margins
and the rounding differ.

## Throughput

Seconds for each load, the median of the runs; less is better. The spread of the runs was
below 2 % for nearly all, and below 5 % for every one.

| Load | Kudari | Alacritty | kitty | Ghostty | Contour | xterm | Konsole | Yakuake |
|---|---|---|---|---|---|---|---|---|
| `plain`: 24 MB of text | 0.17 | 0.17 | 0.20 | 0.17 | **0.16** | 0.56 | 0.74 | 0.73 |
| `color`: 28 MB in 256 colors | **0.13** | 0.14 | 0.23 | 0.26 | 0.50 | 0.45 | 0.47 | 0.47 |
| `unicode`: 20 MB of wide characters, emoji and combining marks | **0.11** | **0.11** | 0.19 | 0.24 | 0.35 | 0.35 | 0.52 | 0.50 |
| `frames`: 300 full screens, every cell in colors of its own | **0.42** | 0.48 | 0.62 | 1.06 | 0.64 | 14.85 | 1.58 | 1.54 |
| `boxes`: 300 screens of panels with lines, blocks and braille | 0.05 | 0.05 | 0.06 | **0.03** | 0.22 | 2.05 | 0.18 | 0.18 |
| `seq`: the numbers up to 2,000,000 | **0.44** | 0.45 | 0.56 | 0.53 | 0.53 | 1.59 | 1.91 | 1.91 |

Kudari parses through Alacritty's terminal library, as Alacritty does, and draws on a
thread of its own, so the two stay close. Against Konsole and Yakuake, which share their
terminal part, Kudari takes in output 3.6 to 4.7 times as fast.

## Processor time under load

Milliseconds of processor time, all threads together, for each megabyte of a load; less is
better.

| Load | Kudari | Alacritty | kitty | Ghostty | Contour | xterm | Konsole | Yakuake |
|---|---|---|---|---|---|---|---|---|
| `plain` | 7.0 | 7.1 | 10.2 | 9.8 | 10.5 | 23.6 | 30.9 | 30.7 |
| `color` | 5.1 | 5.0 | 8.3 | 15.0 | 19.0 | 16.1 | 16.9 | 16.9 |
| `unicode` | 8.8 | 5.7 | 10.3 | 21.9 | 19.6 | 18.0 | 26.6 | 25.4 |
| `frames` | 3.7 | 3.7 | 4.5 | 12.9 | 8.3 | 29.7 | 12.1 | 12.0 |
| `boxes` | 3.9 | 3.3 | 3.3 | 4.1 | 21.5 | 37.6 | 11.8 | 11.6 |
| `seq` | 28.0 | 26.6 | 61.3 | 46.3 | 47.9 | 106.6 | 127.9 | 127.8 |

During the loads `plain`, `color`, `frames` and `seq`, Qt Quick drew 58 to 59 frames a second for
Kudari, at 60 Hz, and 50 during `boxes`. Wide characters and emoji are the exception: there
it drew 38 frames a second, as some frames took up to 64 ms to paint.

## Keys

100 keys, 50 ms apart, each echoed by the shell. Milliseconds from the press of a key in KWin
until the terminal writes it to the shell, over all runs.

| | Kudari | Alacritty | kitty | Ghostty | Contour | xterm | Konsole | Yakuake |
|---|---|---|---|---|---|---|---|---|
| median | 0.08 | 0.05 | 0.05 | 0.10 | 0.05 | 0.05 | 0.09 | 0.08 |
| 95 % of the keys within | 0.12 | 0.07 | 0.07 | 0.36 | 0.07 | 0.07 | 0.13 | 0.12 |
| processor time per key | 0.65 | 0.38 | 0.23 | 0.74 | 0.80 | 0.02 | 0.32 | 0.32 |

Kudari shows the echo in the next frame: 0.6 ms after the key in the median, and 95 % of the
keys within 1.1 ms, with one frame for each key.

## Waiting

Ten seconds without output, with the window shown.

| | Kudari | Alacritty | kitty | Ghostty | Contour | xterm | Konsole | Yakuake |
|---|---|---|---|---|---|---|---|---|
| processor time, ms | 1 | 1 | 4 | 8 | 1 | 0 | 0 | 0 |
| wake-ups a second | 2 | 1 | 15 | 36 | 2 | 0 | 0 | 0 |

Kudari alone, in milliseconds of processor time a second:

| Situation | Processor time | Wake-ups a second | Frames a second |
|---|---|---|---|
| started, the window never shown | 0.03 | 1 | 0 |
| shown, nothing happens | 0.07 | 2 | 0 |
| hidden after it was shown | 0.01 | 0 | 0 |
| shown, with a blinking cursor | 1.21 | 55 | 2 |
| the blinking cursor after 15 s, at rest | 0.08 | 2 | 0 |
| shown, with a fading cursor | 8.51 | 792 | 30 |
| ten lines a second in a tab in the background | 0.35 | 14 | 0 |
| the same, with the window hidden | 0.07 | 10 | 0 |

The cursor does not blink by default, and the fading cursor is an option of its own.

## Memory

Private memory in MB after the loads, and the memory on the graphics card. Kudari keeps an
image of each terminal on the graphics card.

| | Kudari | Alacritty | kitty | Ghostty | Contour | xterm | Konsole | Yakuake |
|---|---|---|---|---|---|---|---|---|
| after typing, with a short history | 70 | 39 | 50 | 78 | 119 | 6 | 47 | 47 |
| after all six loads, with a full history | 157 | 101 | 143 | 131 | 155 | 88 | 51 | 50 |
| on the graphics card | 122 | 48 | 54 | 160 | 210 | – | – | – |

Kudari alone, in MB:

| Situation | Private | Resident | Proportional | Graphics card |
|---|---|---|---|---|
| 1 tab | 59 | 274 | 106 | 115 |
| 5 tabs | 66 | 281 | 113 | 136 |
| 10 tabs | 81 | 296 | 128 | 132 |
| 1 tab, hidden past the time to free its graphics memory | 41 | 258 | 89 | 130 |
| 1 tab with 20,000 lines of history | 123 | 338 | 170 | 132 |

A line of history takes 12 bytes for each column, 3.2 MB for each 1,000 lines of 280
columns. Most of the resident memory belongs to Qt, KDE Frameworks and Mesa, which other
programs of the desktop share; the proportional memory divides the shared part among the
programs that use it.

## Start

Milliseconds from the start of the process, the median of 5 runs.

| Step | ms |
|---|---|
| the name on D-Bus, where a second start finds it | 30 |
| ready, with the QML loaded | 103 |
| the first frame | 108 |
| showing the window the first time, from the shortcut to the frame | 8 |
| showing it again | 3 |
| a second start, which only shows or hides the running Kudari | 29 |
| `kudari --version` | 24 |

## On Windows

`tests/windows/bench.ps1` measures the start, the waiting and the loads of Kudari on Windows,
but not yet next to other terminals. There, the console host of Windows stands between the
shell and every terminal and sets the pace of the output; CONTRIBUTING.md tells how to run
it.

## Repeating the measurement

Build a release of Kudari, then measure it next to every other terminal installed, with the
same font and history:

```sh
cargo build --release
tools/bench.sh --font 'Hack:11' --history 10000
tools/bench.sh --font 'Hack:11' --history 10000 --loads typing,idle
tools/bench.sh --font 'Hack:11' --history 10000 --scenarios
```

The first takes the loads, the second the keys and the waiting, the third Kudari alone. The
report of each starts with the versions and the computer it measured, and warns of a busy
computer or a processor that is not set for performance. `tools/bench.sh` itself describes
its options at its top. This document gets new numbers with the releases that change them.
