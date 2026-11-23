# Accessibility

Kudari should work for everyone who uses a terminal, including people who work with a
keyboard alone, a screen reader, large text or high contrast. This document says what works
today, what does not, how to report a barrier, and what contributors keep in mind. If you
came here because something stood in your way, the section on
[reporting](#reporting-a-barrier) is the place to start.

## What works

- **Keyboard.** A global shortcut shows and hides the window: F12 on KDE Plasma, Ctrl+F12
  on Windows. The actions of the menus can each have a shortcut, and most have one from the
  start: new sessions, splits and moving between terminals, search, the size of the window
  and the size of the font among them. The menu lets you change them: Configure Keyboard
  Shortcuts on Plasma, Shortcuts on Windows. Ctrl+Shift+, on Plasma and Ctrl+, on Windows open the settings. There, Tab
  moves between the search field and the controls of a page; from the search field, the
  arrow keys go through the pages and Return opens one. Open menus follow the arrow keys,
  Return and Escape.
- **Screen readers.** The buttons, tabs and menus of the window and every control of the
  settings have a name and a role. On Plasma, they reach screen readers such as Orca through
  AT-SPI; on Windows, Narrator and other screen readers through UI Automation. The terminal
  itself tells a screen reader that it is a terminal when it takes the focus.
- **Text and colors.** Ctrl++ and Ctrl+- change the size of the font, and the settings take
  any font installed. On Plasma, the colors come from the color scheme of the desktop, high
  contrast schemes included. Kudari moves the colors red to cyan of terminal output closer to
  the text color until they reach a contrast of 4.5:1 against the background, the contrast
  that WCAG asks for normal text. The cursor blinks only if you switch that on.
- **Motion.** On Plasma, KWin slides the window in and out, with the speed of animations set
  for the desktop, and without sliding when its effect is off.
- **Notifications.** Activity, silence and the bell in a session can show a notification of
  the desktop, which screen readers announce.

## Known limitations

- **The text in a terminal is not open to screen readers.** A screen reader learns that a
  terminal has the focus, but cannot read its lines, follow the cursor or read new output.
  For work that needs a screen reader in the terminal itself, Kudari does not help yet.
- **The menus of the bar and of the tabs open with the mouse only.** Their entries have a
  shortcut or can get one, except the color of a tab and About Kudari, which need the mouse
  today.
- **The buttons of the bar are not reachable with Tab.** In a terminal, Tab belongs to the
  shell. Their actions have shortcuts as well.
- **On Windows, the window always slides.** The slide takes 150 ms and does not yet follow the
  setting of Windows that turns animations off. Kudari has not been tried with the contrast
  themes of Windows.
- **No one has tested Kudari with a screen reader for a whole working session yet.** The
  checks so far read what the screen reader gets, as the section on
  [testing](#how-kudari-is-checked) describes. Reports from daily use are most welcome.

## Reporting a barrier

Open an issue with the
[accessibility template](https://github.com/MarcJose/kudari/issues/new?template=accessibility.yml).
It asks for:

- what you tried to do and what went wrong,
- the version (`kudari --version`) and the system, KDE Plasma or Windows,
- the screen reader, magnifier or other assistive technology you use, with its version, if
  any,
- a screen recording or screenshot, only if you are comfortable sharing one.

You do not need to say anything about yourself or a disability. A short report is better than
none. If you prefer not to report in public, write to the maintainer at the address in
[SECURITY.md](SECURITY.md).

### Severity

The maintainer sorts each report by how much it keeps you from your work. You do not need to
choose a severity yourself.

- **Critical:** you cannot do a core task at all, such as opening a terminal or typing in it.
- **Serious:** a task is hard, but there is a way around it.
- **Moderate:** a task is annoying or does not behave the same each time.
- **Minor:** a small issue with little effect on use.

### What happens next

- You get an answer within a week that says what happens next and, where there is one, a way
  around the barrier until a fix comes.
- A critical or serious barrier gets fixed first, in the next release where possible. If that
  is not possible, the issue says why and when to expect a fix instead.
- The issue stays up to date when the plan changes, and you may be asked to check the fix
  before it closes.

## Ownership

The maintainer, Marc Jose, looks after accessibility: sorting reports, tracking known
barriers, and keeping this document current.

## Supported environments

- **KDE Plasma 6 on Wayland,** with Orca as the screen reader.
- **Windows 11,** with Narrator as the screen reader.

Other screen readers that use AT-SPI or UI Automation should get the same information, but
nobody has tried them yet.

## How Kudari is checked

The aim is the level AA of WCAG 2.2, as it applies to software that is not on the web. That
is a goal, not a claim that Kudari meets it: nobody has evaluated Kudari against it.

- The GUI test on Plasma, `tests/gui/run.sh`, reads what a screen reader learns of the window
  and of every page of the settings. It checks that the terminal has its role, that every
  button, field, list and switch has a name, and that no name or description carries markup.
- The GUI test on Windows, `tests/windows/gui.ps1`, uses Kudari through UI Automation, as a
  screen reader does: it finds the tabs, menus and settings by their names and uses them.
- A unit test checks that the colors of terminal output reach their contrast against the
  background.

## For contributors

- Give every control a name a screen reader can read: a button that shows only an icon or a
  glyph needs `Accessible.name`. The GUI test fails when a control has none.
- Every new action gets a place among the shortcuts, so that it works without the mouse.
- Do not let color alone carry a meaning; add text, an icon or a shape.
- Keep markup out of the texts of controls; the QML pages translate them as plain text.
- For a change to the user interface, try it with the keyboard alone, and if you can, with a
  screen reader: Orca and Accerciser on Plasma, Narrator and Accessibility Insights on
  Windows. Say in the pull request what you tried.

## Feedback

Ideas for this document or for how Kudari handles accessibility are welcome as an issue or a
pull request. Barriers go through [the report above](#reporting-a-barrier).
