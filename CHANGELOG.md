# Changelog

All notable changes to Kudari. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
From 0.1.0 on, `tools/release.py` writes each section from the [Conventional Commits](https://www.conventionalcommits.org) since the previous release.

## [0.1.0] - unreleased

The first release: a drop-down terminal for KDE Plasma on Wayland in Rust, cxx-qt, QML and
Kirigami, which keeps the workflow of Yakuake.

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
- A German translation, a man page, and notifications through KNotification.
