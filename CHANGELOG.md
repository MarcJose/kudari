# Changelog

All notable changes to Kudari. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
After 0.2.0, `tools/release.py` writes each section from the [Conventional Commits](https://www.conventionalcommits.org) since the previous release.

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
