# Architecture

Kudari runs on KDE Plasma and on Windows, each time with the look and the means of the
system: Qt Quick with Kirigami and KDE Frameworks on Plasma, Qt Quick with the FluentWinUI3
style and Win32 on Windows. GTK and macOS may follow later, X11 is not planned. This
document describes the layout of the code and the steps that got it there, each of which
left a working, fully tested Kudari behind.

## Layers

```
kudari-core     plain Rust: no Qt, no windows, no platform calls outside traits
  terminal       emulation (alacritty_terminal) and PTY; process information behind a trait
  input          keys and mouse in Kudari's own types, turned into bytes for the program
  view           selection, selection mode, search, links, hints, scrolling, in cells
  render         a frame per terminal: rows with resolved colors and styles, cursor,
                 underlines, box drawing, markers; a front end only paints it
  app            the logic behind the controller: sessions, monitors, bell, auto save,
                 actions, window state, the access of AI agents
  settings       the model and a storage trait
  agent, mcp     the rules for AI agents and the protocol, without a transport
kudari (KDE)    Qt Quick, Kirigami, the KDE Frameworks bridge, layer shell, D-Bus, KConfig
kudari-qt       the Qt parts both front ends share, above all the terminal item
kudari-windows  FluentWinUI3 pages, Win32 services, a named pipe, an INI file
```

The front end sends the core commands, such as a key press, closing a tab or a changed
setting, and gets effects back, such as new tabs, a terminal to repaint, a notification or
a question for the user. Questions are asynchronous: no dialog blocks the core, which could
otherwise leave the controller in use twice during a nested event loop. Tests can run the
core without Qt, so logic that only the GUI test reached before has unit tests.

The KDE front end is the package at the root of the workspace, so the build, the QML, the
C++ bridge and the translations keep their places. The Qt parts that the Windows front end
shares with it are in `kudari-qt`.

## Steps

Every step is one or a few commits. After each, the KDE version behaves as before, the unit
and GUI tests pass, and the coverage stays at 90% or more.

1. **Workspace.** `crates/core` holds the modules that are neutral already: sessions,
   keytab, box drawing, colors, the INI reader, the settings model with a storage trait,
   and the random input for tests. KConfig becomes one implementation of the storage.
2. **Input.** Own key and modifier types replace Qt's key codes. Key sequences, the kitty
   keyboard protocol, the keypad, mouse reports and the preparation of pasted text move
   into the core; Qt only translates its events.
3. **View.** Selection, selection mode, search, links and scrolling move from the terminal
   item into the core and work in cells instead of pixels.
4. **Frame.** The core resolves colors and gives the shapes of cursors, underlines and box
   drawing; the terminal item paints the frame with `QPainter` and nothing else.
5. **App.** The logic of the controller moves into `core::app` with commands and effects;
   the Qt controller becomes an adapter. Dialogs become asynchronous.
6. **Services.** Traits for the global shortcut, notifications, the clipboard, opening
   links, theme colors, window placement, and a hook for translations.
7. **Agents.** The agent interface and `kudari mcp` get a transport trait; D-Bus stays in
   the KDE front end.
8. **Operating system.** Process information and signals go behind a trait. The CI builds
   and tests the core on Windows as well, so Unix assumptions show up early.

All eight steps are complete, and the Windows front end below builds on the same core.

## Windows

The Windows front end offers what the KDE one does, in the way Windows does it. What has no
counterpart there stays out: the D-Bus interface for scripts, the import from Yakuake and
the effects of KWin. It needs Windows 11: Windows 10 gets no updates since October 2025.

### Layout

```
crates/core      unchanged: everything without Qt
crates/qt        kudari-qt: the terminal item, the Qt keys, the terminal backend, the
                 controller, a C++ bridge for plain Qt (menus, file and print dialogs,
                 badge, fonts) and the QML of the terminal pane
(root)           kudari: Kirigami pages, KDE Frameworks, D-Bus, KConfig
crates/windows   kudari-windows: Fluent pages, Win32, the named pipe, an INI file
```

The controller is a QObject of cxx-qt and cannot be generic, so it reaches its front end
through a trait object set at startup: the services of step 6, and what only the shell of a
front end has, such as menus, actions with their shortcuts, the tray and the dialogs. The
QML of KDE imports Kirigami, so the terminal pane in kudari-qt does without it and takes its
colors and spacing from a small theme object instead, while tabs and window have their own
QML per front end. The front ends share the settings pages as well, with the parts of their forms
per front end (step 9 below).

### Steps

Each step leaves the KDE version as it was, with all its tests passing.

1. **The last dialogs.** Saving the output, printing and the size of the scrollback still
   open dialogs with a nested event loop. They become asynchronous like the questions, since
   both front ends share the controller.
2. **kudari-qt.** The shared Qt parts move out of the KDE package, and the controller gets
   its trait object. The KDE version behaves as before, which the GUI test shows.
3. **The terminal on Windows.** The backend builds on Windows, with ConPTY through
   `alacritty_terminal`. `process::windows` implements `Programs` from the process tree
   (`CreateToolhelp32Snapshot`): busy while the shell has children, the name of the newest
   one. Only interrupting (Ctrl+C), terminating and killing exist there, so `Programs` says
   which signals it supports and the menu shows only those. Flow control does nothing. A
   reader between the PTY and the emulator picks up OSC 7 and `OSC 9;9` for the working
   directory and OSC 133 for the prompt. Linux keeps `/proc` and uses the same marks where
   a shell sends them.
4. **Shells.** New tabs start Command Prompt, Windows PowerShell 5, PowerShell 7 or a WSL
   distribution (from `wsl.exe --list --quiet`), and the setting names the default. Kudari
   sets up shell integration itself: a `PROMPT` with OSC 133 and `OSC 9;9` for the Command Prompt,
   and a prompt function for PowerShell. Their OSC 133 marks carry a random key of the
   terminal, and where a terminal has a key, marks without it do not count: a file shown with `cat`
   or `Get-Content` cannot fake a prompt. A setting turns it off.
5. **A window.** Kudari for Windows opens a borderless, topmost window with the
   FluentWinUI3 style, on the screen under the pointer and within its work area, with tabs,
   splits and terminals. It slides in through a QML animation, as there is no compositor
   effect for it. Settings live in `%APPDATA%\Kudari\kudari.ini`, which needs a writer
   next to the INI reader of the core. The translations of `po/` go into the binary, read by
   a small gettext reader in the core that knows contexts.
6. **Services.** The global shortcut through `RegisterHotKey`. F12 belongs to the debugger,
   so the first start asks for another key. The clipboard of Qt, without a primary
   selection. Toast notifications, which need the identity of the MSIX package, falling back
   to the tray. Links through `ShellExecuteW`. Accent color and light or dark mode from the
   settings of Windows, Mica or Acrylic through `DwmSetWindowAttribute` on Windows 11. A
   tray icon, a single instance, and a second start that toggles the window.
7. **Agents.** The agent interface goes over a named pipe for the user only: a security
   descriptor with the user's SID, `PIPE_REJECT_REMOTE_CLIENTS`, and
   `FILE_FLAG_FIRST_PIPE_INSTANCE` against a program that opens the name first. One
   connection is one agent, and `GetNamedPipeClientProcessId` names its program for the
   question, which the window shows in its QML with a button in the tab bar that ends the
   access. `kudari mcp` connects with `SECURITY_IDENTIFICATION`, checks that the
   server runs as the same user, and implements `mcp::Instance` over the pipe. The core
   has the pipe in `pipe` and the lines that cross it in `remote`, with a Unix socket for
   the same elsewhere. PSReadLine has no bracketed paste, so while commands wait for the
   user's Return, an agent types only between the signed OSC 133 marks of the command line
   and of the command. The vi mode of PSReadLine gets no mark of a command line, as keys act
   there instead of going into it. Without the
   shell integration of Kudari, an agent cannot type.
8. **Packages.** The program, `kudari.exe` in the packages and `kudari-windows.exe` as cargo
   builds it beside the `kudari` of KDE, is a program without a console, so that none opens with
   the window. `mcp`, `--version` and `--help` write to the console they came from.
   `tools/windows-package.ps1` gathers it with Qt (`windeployqt`), the runtime of Visual
   C++ and the licenses, signs it, and makes a ZIP archive and an MSIX package with the
   alias `kudari`. The certificate is self-signed for now: the CI signs with a throwaway
   one and installs the package, a release with the one in its secrets. winget installs
   the ZIP archive, as it trusts no self-signed package. The SBOM of the crates gets Qt and
   the runtime added. `tests/windows/gui.ps1` drives the window through UI Automation, as a
   screen reader would: tabs, menus, the about dialog, every page of the settings, a switch
   there, and quitting.
9. **Parity with KDE.** What the KDE version offers in its window, Windows offers as well:
   the grip below the tab bar, a tab bar that does what the one of KDE does, and the
   settings. Their pages live in kudari-qt and are the
   same for both. They import the directory `qrc:/qml/form`, where each front end puts the
   parts of its forms with the API of Kirigami Addons' FormCard: thin wrappers around it on
   KDE (`qml/form`), Fluent controls on Windows (`crates/windows/qml/form`). `tests/check.sh`
   lints the pages against both. What belongs to one front end only, such as themes, D-Bus
   or the choice of shells, the pages show where `Controller::supports` says so. The
   shortcuts have a page of their own on Windows, with the global one, and the first start
   offers another global one, as F12 alone belongs to the debugger there. Where KDE has
   themes, Windows lets the user choose light or dark instead of following the system,
   through `QStyleHints::setColorScheme`. The menus of the bar are windows of their own
   (`Popup.Window`), so that they reach beyond the window, and open upwards at the bottom
   of the screen.

All nine steps are complete.

### Decisions

- The global shortcut is Ctrl+F12 by default. F12 alone belongs to the debugger, Win+` to
  the drop-down window of Windows Terminal, and Ctrl+` to the terminal of VS Code. The first
  start offers another key.
- Windows 11 only, as Microsoft no longer updates Windows 10.
- A certificate of Kudari's own signs the packages, without costs or a third party. Users
  trust it once for the MSIX package. winget installs the ZIP archive, as it trusts no
  self-signed package.
