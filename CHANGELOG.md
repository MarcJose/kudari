# Changelog

All notable changes to Kudari. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
After 0.2.0, `tools/release.py` writes each section from the [Conventional Commits](https://www.conventionalcommits.org) since the previous release.

## [0.5.0] - 2026-11-22

Terminals are drawn on the graphics card, output is parsed faster, and on Windows output comes
through many times faster with the console host of Windows Terminal. A panic or costly output
no longer stops all terminals, and links, titles and agents show more of what they are.

### Features

- Terminals are drawn on the graphics card by default: the fills and the glyphs of a frame,
  from an atlas, the badge, translucent terminals and scaled frames. A full screen of colors
  needs 2 to 3 ms in the thread of the window instead of 8 to 9, and 13 to 14 at a scale
  of 2. "Draw terminals:" on the page Appearance of the settings chooses the processor.
- A link of a program whose text says something else shows where it leads in a hint, and
  the menu of the terminal, also on Windows, starts with its address.
- The page Terminal of the settings tells what the history of a terminal takes, and warns
  where all terminals would take more than a quarter of the memory of the computer. The
  dialog for the lines of one terminal tells it as well.
- A new setting limits the frames a second that output alone gets, to 60 or 30, for screens
  faster than that. What is typed still shows at once.
- A new setting lets only the terminal in front copy to the clipboard with OSC 52.
- A new setting gives the graphics of a window hidden for a while back, after 60 s by
  default.
- On Windows, the messages of Kudari from its package are notifications of Windows, in the
  group of their event. Where Windows refuses the global shortcut as Kudari starts, it says
  so, and a key of the number pad can be the shortcut.
- On Windows, the question about an AI agent names where its program lies, and says where
  that is a folder for downloads or temporary files.

### Security

- Links open no desktop entry, by any name or by what is in the file.
- Titles from programs, file names and scripts show no marks of the direction of text, and
  a hint over a link shows hidden characters as a sign.
- A call of an AI agent starts no Kudari, and waits at most 10 s for an answer.

### Fixed

- A panic ends only its terminal, not all of Kudari.
- Costly output, as long repeats or a flood of screen alignments, no longer freezes every
  terminal, and Ctrl+C reaches the program while its output is parsed.
- On Windows, a terminal no longer stops for good when the console host sends more than
  64 KiB at once, as a full screen in colors.
- The copies of the emulator end no terminal for a full stack of keyboard modes of kitty, a
  wide character below the scrolling region or a console that fails to start, and erase,
  copy and keep wide characters as they should.
- A search ends, also for empty matches and wide characters, finds matches where it cannot
  go around them, and highlights them in the text the frame paints.
- Copying takes wide characters that wrapped and lines that start with half of one.
- What an input method composes shows as plain text.
- Terminal images lie on whole pixels of the device.

### Performance

- Output is parsed faster: ASCII without decoding, the parameters of an escape sequence in
  one go, and runs of plain text into the grid a line at a time. Plain text takes 43 % fewer
  instructions in the parser, a full screen of colors 31 % fewer.
- A frame paints, uploads and builds only the lines that changed: a changed line of status
  costs half the processor time, a typed key 28 % less, and up to 77 % less at a scale of 2.
- On Windows, the packages carry the console host of Windows Terminal: plain text comes
  through 19 times as fast, the numbers to 2,000,000 50 times, and ten terminals take
  100 MB less memory. Each terminal takes 1.1 MB less for its pipes, the first one
  starts without waiting for the list of WSL, and settings go to the file a moment after
  they change.
- A hidden window polls every 4 s instead of twice a second, and finds the program in the
  foreground of a terminal once a poll.
- Memory goes back after closing a terminal or clearing its history (126 MB → 60 MB after
  closing a tab with 20,000 lines), and a wider terminal keeps its history at the size of
  its width (3 columns wider took 96 % more memory, now 2 %).
- Searching while typing finds the newest match nearby first; the marks on the scrollbar
  are found once a pixel row. Moving the mouse looks for links only in a new cell.
- Automatic saving of output works only after changes, off the thread of the window; the
  grip of the window and the settings are saved once they settle.

### Development

- `tools/bench.sh` times loads up to an answer of the terminal and has loads of wide
  characters, panels, a status line, typing, scrolling, searching, floods of titles and
  queries and zooming; it measures the graphics card, the same font and history for all
  terminals, other rates of the screen and a second screen.
- `tests/windows/bench.ps1` measures the Windows front end: start, waiting, memory and output
  through ConPTY.
- The copies of `vte` and `alacritty_terminal` are tested against the originals, and the GUI
  test runs with the C++ under AddressSanitizer and UndefinedBehaviorSanitizer.

## [0.4.0] - 2026-11-18

Less work for every frame, above all at a scale of 2 and with a blinking or fading cursor,
and AltGr on Windows types its characters again.

### Fixed

- On Windows, AltGr types its characters in shells of WSL: AltGr and + on a German keyboard
  typed no tilde, as Kudari sent Escape before it as for Alt. With the link numbers on
  Ctrl+Alt, AltGr and 7 to 0 type `{ [ ] }` instead of opening a link.
- A fading cursor fades over its cell with the character in it, rather than into the
  background of the color scheme.

### Performance

- A terminal shows its image as a texture of its own: no frame is converted, copied or
  cleared on its way to the screen any more. Taking in output costs 7 to 12 % less CPU time
  at a scale of 1 and 21 to 43 % less at a scale of 2, where a full screen of colors now
  draws 58 frames a second instead of 47. Each window keeps 14 MB less memory.
- The cursor has an image of its own: blinking and fading only change how much of it
  shows. A blinking cursor costs 1.2 ms of CPU a second instead of 7.1 in 0.3.0, a fading
  one 12 ms instead of 99.
- A terminal refreshes once a frame rather than for every piece of output: with seq, 48
  times a second instead of 3666. A full screen of colors comes in 6 % faster.

### Development

- `tools/bench.sh --scenarios` measures the start, showing the window, a second start,
  waiting and memory; `--perf` profiles the loads; several builds compare in the same runs
  with a verdict.
- A build with the feature `stats` counts what Kudari does, for the bench and the GUI test,
  which now checks that nothing is painted while nothing happens and waits until Kudari has
  settled instead of fixed pauses.
- `cargo bench -p kudari-core` counts instructions, and `tools/bench-compare.py` compares
  them between two states.

## [0.3.0] - 2026-11-17

Faster and leaner drawing, a cursor that rests, a safer Windows build, and 137 fixes
from a review of the whole code.

### Security

- On Windows, Kudari loads libraries only from its own folder, from System32 and from the
  folder of Qt. Before, a `conpty.dll` in the folder Kudari was started from, or in any
  folder of PATH, ran inside Kudari with every new terminal.
- Output can no longer end Kudari with all its terminals: a view that output scrolled past
  the history, and the cursor of the selection mode left outside the grid after a switch
  to the alternate screen, are put back before they are read, and a pane keeps at least
  two columns.
- Output can no longer take all memory: an OSC sequence without an end stops at 8 MiB for
  a text to copy and at 64 KiB for anything else, combining marks on a cell are capped,
  and the answers that wait for the window are bounded. A copy beyond the limit copies
  nothing rather than its start.
- Titles that programs set show as text, never as markup.
- A call over D-Bus never runs after its caller heard that it timed out.

### Features

- A blinking cursor rests, steady and shown, after 15 seconds without input or output; the
  new setting "Stop blinking after" changes the time. It blinks in the cycle of the system,
  stays steady where the system turned blinking off, and starts anew with every key.
- The kitty keyboard protocol sends the code of the base key for a shifted key and the text
  of a key, and reports only the flags that Kudari carries out.

### Performance

- Terminals in tabs in the background are drawn only when they show, and give back their
  image and texture meanwhile: some 14 MB of memory and more on the graphics card for each
  tab at 2560×1440. A hidden window draws nothing either.
- A cursor that blinks or fades draws nothing while a program hides it or the view is
  scrolled away from it, nor once it rests; a fading cursor used to cost a tenth of a core.
- Frames take less work: glyphs are drawn in runs for each font and color, into an image
  that Qt neither clears nor blends; cells of one background are one rectangle; no string
  is made for each cell; and a key draws one frame, with its echo, instead of two. At a
  scale of 2, the frames that came late during heavy output fell from about half to almost
  none.
- On Windows, the terminals share one look at the processes: an idle Kudari with ten tabs
  went from about a tenth to a fiftieth of a core.

### Bug fixes

- 137 fixes from a review of the whole code, among them: the app logic and the window,
  the search and the scrollbar, links and their numbers, mouse, keyboard and the selection,
  copying as HTML, settings and titles, tabs and monitors, D-Bus and agents, the import
  from Yakuake and Konsole, the dialogs and translations, and the Windows front end.
- Lines of tables are as wide in every cell at a fractional scale such as 125 %.
- A tab in the background keeps its size and wraps nothing on a switch of tabs.
- Calls over D-Bus that arrive before the window is ready are kept, and the window shows
  again with the next shortcut after its screen went away.

### For developers

- `tools/bench.sh` names the build and the computer it measured, cleans the environment,
  and reports the median and spread of its runs with a warm-up run and a shuffled order;
  for Kudari, the times of its frames, the CPU time of each thread, how often it wakes up,
  its memory, and a terminal that waits (`--loads idle`, `--set`, `--detail`).
- Release builds compile the C++ parts with `QT_NO_DEBUG`, through a fix in cxx-qt that is
  part of its next release.
- Rust 1.100 and updated dependencies; Kudari builds with KDE Frameworks 6.32.

## [0.2.0] - 2026-11-13

The first release: a drop-down terminal for KDE Plasma on Wayland and for Windows 11, in
Rust, cxx-qt and QML, which keeps the workflow of Yakuake.

### Features

- A window that slides down from the top edge on a global shortcut, F12 by default, with
  width, height, screen and position to choose, and a grip to change the height.
- Sessions as tabs in a single bar with Plasma's own controls, each divided into several
  terminals side by side or stacked, with splitters to drag and tabs to move.
- Terminals on `alacritty_terminal`: colors, fonts and icons follow the Plasma theme;
  search, selection mode, scrollback markers, links to URLs and files, a badge, margins
  and a vertical line; Konsole's `.keytab` key bindings.
- Monitors for activity, silence and finished processes, also as one-shot monitors, and
  locks against closing a session or typing into it.
- Yakuake's D-Bus interface under the name `io.github.marcjose.kudari`, so scripts carry
  over; running commands over D-Bus is off unless switched on.
- A first-start dialog for the shortcut, which takes over the settings of Yakuake and of
  Konsole's default profile on request, reading their files only.
- `kudari mcp`, a Model Context Protocol server through which AI agents read the
  terminals and, if allowed, type commands that wait for the user's Return. It is off by
  default, and Kudari asks the user before each new agent gets access.
- A German and a British English translation, a man page, and notifications through
  KNotification.
- A version for Windows 11 with the same settings in the style of Windows, its accent color
  and light or dark mode: Ctrl+F12 by default, tabs and split terminals for the Command
  Prompt, PowerShell and WSL, as an MSIX package, a ZIP archive and manifests for winget,
  signed with a certificate of its own. Its menus name their shortcuts, and its bar has the
  order of the KDE version.
- Packages for Arch Linux in `packaging/aur`: `kudari-bin` from the release and `kudari-git`
  from `main`.
- Releases with checksums signed through Sigstore, build provenance, SBOMs, and a Linux
  package that is rebuilt and compared byte for byte before it is published.
