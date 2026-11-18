# Contributing to Kudari

Thank you for helping. Bug reports, ideas, translations and code are all welcome. Everyone
taking part follows the [Code of Conduct](CODE_OF_CONDUCT.md). Security problems go through
[SECURITY.md](SECURITY.md), not public issues.

## Issues

A bug report needs the version (`kudari --version`), the Plasma and Qt versions, and the
steps that show the problem. Kudari shows its messages when you start it from another
terminal with `QT_LOGGING_RULES='*.warning=true'`. For a larger change, open an issue
first, so that you can agree on the way before you spend time on it.

## Setting up

Kudari builds on Linux with Rust, Qt 6 and KDE Frameworks 6. `.github/arch-packages.txt`
lists the packages the CI installs on Arch Linux, and other distributions name them
similarly. `README.md` shows the command for Arch. With `lld` installed, the build links
with it. `tests/check.sh` needs some tools more: [Vale](https://vale.sh) for the English
of the texts, `namcap` for the packages of the AUR, `g++` for the warnings of the C++ code,
`qmllint` of Qt for the QML, and `checkXML6` of KDocTools with `xsltproc` for the man page.
On Arch: `sudo pacman -S --needed vale namcap gcc libxslt`.

For development, Cargo builds and runs Kudari directly. If the source lies in a synced
folder, keep the build output out of it:

```sh
export CARGO_TARGET_DIR=~/.cache/kudari-target
cargo run
```

The window starts hidden. F12 brings it down, and starting the binary a second time toggles
the running instance. Translations, icons, notifications and the D-Bus service need an
installation, as `README.md` describes.

Turn on the Git hooks once, so the checks run before each commit and push:

```sh
git config core.hooksPath .githooks
```

## Making a change

- **Tests.** `tests/check.sh` has to pass: formatting, clippy and C++ warnings, unit tests,
  translations, licenses and the version. New behavior gets a unit test, or a check in the
  GUI test `tests/gui/run.sh` when it needs the window. Coverage of the Rust code stays at
  90 % or more, as `tools/coverage.sh` measures it.
- **Code.** `cargo fmt` formats, and clippy and the C++ compiler run with warnings as errors.
  Code, comments and commit messages are in English. Comments say why, not what. Vale checks
  the English of the documentation, the man page and the comments against the Google
  developer documentation style, with the exceptions in `.vale.ini`. Words of the project
  that it does not know go into `.vale/styles/config/vocabularies/Kudari/accept.txt`.
- **Layout.** What depends on no user interface and no platform belongs in `crates/core`.
  What needs Qt but not KDE, such as the terminal item and the controller, belongs in
  `crates/qt`, and the package at the root is the KDE front end. The core reaches the clipboard,
  the theme, the screens and other services of the platform through the traits in
  `crates/core/src/services.rs`, and `crates/qt` reaches the menus, actions and questions of
  KDE through `crates/qt/src/frontend.rs`, and `src/plasma.rs` implements both. What it asks of
  the operating system, such as the programs in a terminal, goes through
  `crates/core/src/process.rs`. The core builds and passes its tests on Windows too, which
  the CI checks. `docs/architecture.md` describes where the code is going.
- **User interface.** Kudari follows KDE's Human Interface Guidelines and builds its
  interface in QML and Kirigami. It reaches KDE Frameworks without Rust bindings through
  `cpp/`, and plain Qt through `crates/qt/cpp/`. Other front ends share the terminal pane in
  `crates/qt/qml/`, so it uses no Kirigami and takes its colors and spacing from
  `PaneTheme`.
- **Texts.** Every text the user sees is translatable and needs a translation in every
  language under `po/`, and `tools/translations.py` shows what is missing. If you cannot
  translate a new text, say so in the pull request, and the maintainer adds the translations
  before the merge.
- **Generated files.** `tools/box_drawing.py` writes the tables in
  `crates/core/src/linedraw.rs`, and `tools/third_party_licenses.py` writes
  `THIRD_PARTY_LICENSES` when the crates change. `tools/icons.py` renders the icon,
  `data/icons/io.github.marcjose.kudari.svg`, into the files Windows needs, which
  `--check` compares with the SVG. KDE installs the SVG and its symbolic variant itself.
  `tools/screenshots.sh` takes the screenshots of the KDE version in `docs/screenshots`,
  from a build with `--features test-hooks`. Those of Windows need taking by hand.
- **Dependencies.** A new crate needs a good reason. `cargo deny check` allows only the
  licenses listed in `deny.toml` and crates from crates.io.

## The checks

```sh
tests/check.sh                                   # translations, formatting, lints, prose, unit tests
cargo build --features test-hooks && tests/gui/run.sh
tools/coverage.sh --html                         # coverage of unit and GUI test together
cargo clippy -p kudari-core --all-targets --target x86_64-pc-windows-gnu  # the core on Windows
cargo build -p kudari-windows && tests/windows/run.sh  # the Windows front end, on Linux
tools/windows-package.ps1 -Out dist && tests/windows/package.ps1 dist  # the packages, on Windows
tests/windows/gui.ps1 path\to\kudari.exe  # the window, on a Windows desktop
tools/security.sh                                # secrets, dependencies, workflows
tools/bench.sh                                   # speed of output and drawing, against other terminals
tools/bench.sh --scenarios                       # start, showing, waiting and memory of Kudari
tools/bench.sh --perf                            # the same loads under perf, with a profile of each
tools/bench.sh --rev main --binary new=PATH      # two builds of Kudari in the same runs, with a verdict
cargo bench -p kudari-core --bench core          # scanner, parser and frames alone, without a window
cargo bench -p kudari-core --bench allocations   # the allocations of the same frames and cases
tools/bench-compare.py                           # the core benchmark, main against the working tree
```

`tools/bench.sh` measures Kudari as a whole in a virtual KWin, next to other terminals.
To compare a change with what came before, give `tools/bench.sh` both builds. Each
`--binary NAME=PATH` and each `--rev [NAME=]REV` runs as a terminal of its own in the same
runs. A `--rev` is built in a worktree under `~/.cache` and kept for the next time:

```sh
tools/bench.sh --rev main --binary new=target/release/kudari --runs 10
```

The report compares each build with the first, load by load. It gives the time and the CPU
time, their ratio, a bootstrap interval of the ratio, and a verdict. A build is faster or
slower only where the interval leaves out 1; a difference of a few percent needs ten runs or
more. A verdict is about the two builds, not always about the change: where the code lies in
the program alone can move a load by five percent, even between builds whose code differs
only outside it. Check a gain of the parser or the drawing with the instructions that
`tools/bench-compare.py` counts, which the layout does not change.

`tools/bench.sh --scenarios` measures Kudari alone, in the same virtual KWin, when it is not
taking in output:
- how long it takes to start, with the phases it marks itself with `KUDARI_TIMING=1`;
- how long it takes to show its window;
- how long a second start and `--version` take;
- its CPU time, how often it wakes and its frames while it waits: shown, hidden, with a
  blinking or fading cursor, and with output in a tab in the background;
- its memory with 1, 5 and 10 tabs.

`cargo bench -p kudari-core --bench core` measures the parts without a window: the scanner of
the marks and the parser of the emulator over the same loads, and the paints of a frame for
a few kinds of screen, and smaller cases one by one: the words for the numbers of links, a link
and a color under the mouse, box drawing, keys, a search through a long history, the copy as
HTML and the wrapping on a resize. It takes seconds and needs no KWin, so a change to one of
those parts can be measured before and after at once; `-- throughput`, `-- frame`, `-- cases`
and `-- --runs N` choose. `--bench allocations` counts the allocations of the same frames and
cases; counting needs an allocator of its own, which would make the times wrong, so it is a
benchmark of its own. Benchmarks build with thin LTO, so that their times do not depend on
the other code in them. Compare figures only from the same computer.

On Linux, the core benchmark also counts the instructions of each figure, which vary far
less than times: by less than a hundredth of a percent for a frame. `tools/bench-compare.py`
builds the core benchmark of a commit, the merge base with `origin/main` by default, and of
the working tree, runs both in turn and marks what changed: instructions by more than half a
percent, or times by more than five where nothing could be counted. A change to the drawing,
the parser or the scanner shows there in a minute, before a run of `tools/bench.sh`.

A build with the feature `stats` counts what Kudari does: how often the PTY thread wakes it,
refreshes and paints of each terminal, the wait for and the hold of the lock of a terminal,
the wait in the queue of the Qt thread, and the build, draw and copy of a frame. With
`KUDARI_STATS=1` it writes them to stderr once a second, or every `KUDARI_STATS_MS`
milliseconds. `tools/bench.sh` sets both, so that the report of a build made with
`cargo build --release --features stats` shows what Kudari counted during each load. The test
build has them as well: the GUI test reads them over D-Bus to check that nothing is painted
while nothing happens, and waits with them until Kudari has settled. Without the feature,
nothing is counted.

### Profiling

To see where the time goes, build Kudari with the profile `profiling`. It is the release
build with the lines of the code in its symbols and with frame pointers, so that `perf` can
follow the calls through Rust and Qt. It goes to a directory of its own, because the flags
differ:

```sh
RUSTFLAGS="-C force-frame-pointers=yes" CXXFLAGS=-fno-omit-frame-pointer \
    CARGO_TARGET_DIR=~/.cache/kudari-profiling cargo build --profile profiling
tools/bench.sh --binary ~/.cache/kudari-profiling/profiling/kudari --perf --runs 1 --warmup 0 kudari
```

With `--perf`, `tools/bench.sh` starts Kudari under `perf record`, which samples the cycles
and calls in user space only. `perf` is the parent of Kudari, so this needs neither the rights
to trace another process nor a lower `perf_event_paranoid` than 2. The data is kept, and the
report ends with a command of `perf report` for each load, which shows only the time of that
load:

```sh
perf report -i …/perf-kudari-1.data --time 56091.98,56092.69 --sort comm,dso,sym
perf report -i …/perf-kudari-1.data --time 56091.98,56092.69 --comm 'PTY reader' --no-children
```

The threads to look at are `PTY reader` (the parser), `QSGRenderThread` (drawing),
`Thread (pooled)` and `main`. The functions of Kudari, `alacritty_terminal` and `vte` all have
their names. Frames of the libraries of the system, such as `libc` and Qt, show as
`[unknown]` without their debug information. With `DEBUGINFOD_URLS` set, `perf` fetches it; it
then sends the build IDs of those libraries to that server. Under `perf`, the build of
`profiling` takes in output within a few percent of a release build. A build of `profiling`
must never be published.

`--call-graph fp` keeps only the addresses of the calls. `--call-graph dwarf` would copy
pieces of the stack into the data, and with them what the terminal showed; the loads of the
bench are made up, but keep that in mind for anything else. For memory, `heaptrack` runs
Kudari the same way: `heaptrack ~/.cache/kudari-profiling/profiling/kudari`.

On Windows, `tools/windows-package.ps1` builds the Windows front end and packs it, as the
release workflow does. It needs PowerShell 7, Git, Rust, Python, the Visual Studio Build
Tools with the C++ tools and the Windows SDK, and Qt for MSVC:

```powershell
winget install Microsoft.PowerShell
winget install Git.Git
winget install Rustlang.Rustup
winget install Python.Python.3.13
winget install Microsoft.VisualStudio.2022.BuildTools --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
python -m pip install aqtinstall
python -m aqt install-qt windows desktop 6.8.3 win64_msvc2022_64 --outputdir C:\Qt
# in every new shell, in pwsh:
$env:QMAKE = 'C:\Qt\6.8.3\msvc2022_64\bin\qmake.exe'
$env:Path = "C:\Qt\6.8.3\msvc2022_64\bin;$env:Path"
```

Without `-Certificate`, the script signs with a throwaway certificate. For packages like
those of a release, it takes the real one, with its password in the environment:
`$env:KUDARI_CERTIFICATE_PASSWORD = Get-Content password.txt` and then
`tools/windows-package.ps1 -Out dist -Certificate signing.pfx -Timestamp`.
`tests/windows/package.ps1` installs the package and removes it again. It needs an
administrator shell because it trusts the certificate for the whole computer, and it also
removes an installed Kudari, so it belongs in a virtual machine. `tests/windows/gui.ps1`
needs a desktop session and no other running Kudari. It starts the program with a
configuration of its own and in English, and drives it through UI Automation.

`tools/translations.py` compares the translatable texts in the code with `po/` and shows per
language which texts have a translation, which miss one or have a fuzzy one, and which
entries the code no longer uses.
`--verbose` names every gap, `--prune` removes unused entries. `tests/check.sh` runs it with
`--check --strict`: every text needs a translation in every language, and unused entries and
calls whose text it cannot read fail as well. Without `--strict`, it only reports gaps in
single languages.

`tests/gui/run.sh` runs Kudari in a virtual KWin on a private D-Bus session and drives it
with real input. It touches neither the desktop nor your configuration. The `test-hooks`
feature adds the D-Bus object it needs and must stay off in normal builds.

`tools/coverage.sh` measures with `cargo-llvm-cov` which lines of the Rust code the unit tests
and the GUI test run, taken together. It does not measure the C++ bridge and QML. The CI
shows the table in the summary of the run, attaches the HTML report, and fails if the
coverage drops below 90 %.

`tools/security.sh` runs the checks of the security workflow: `gitleaks` and `trufflehog`
look for secrets in the whole history, `cargo-deny` and `osv-scanner` for vulnerable or
malicious crates, unwanted licenses and crates from elsewhere than crates.io, and
`actionlint` and `zizmor` for mistakes and weak spots in the workflows, and `shellcheck` for
the shell scripts. It skips tools that
are not installed and names them at the end. The workflow also runs weekly, so that it finds
new advisories without a push. Dependabot proposes updates only a week after their release,
which keeps out most malicious versions. Security fixes come at once.

With the Git hooks turned on, the quick checks run before a commit: secrets in the staged
changes, formatting, translations, generated tables and licenses. Before a push,
`tests/check.sh` and `tools/security.sh` run, and the CI takes over the GUI test. The hooks build
in `~/.cache/kudari-target` unless `CARGO_TARGET_DIR` says otherwise. `--no-verify` skips
them once.

## Commits and pull requests

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org), because
`tools/release.py` makes the changelog and the next version from them:

```
type(scope): what the change does, in the present tense

Why it is needed, and what a reviewer should know.
```

The types are `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `build`, `ci`, `chore`,
`style` and `revert`. The scope is optional and in lower case, such as `tabs` or `dbus`. A `!`
after the type or scope marks a change that breaks settings, the D-Bus interface or the
command line. All but `test`, `ci`, `chore` and `style` appear in the changelog.

The title of a pull request follows the same form, since it becomes the commit when the
maintainer squashes the pull request. A workflow checks the title and every commit, and the
commit-msg hook does the same locally. Keep a pull request to one topic, and describe how you tested it.

Branch off `main` and open the pull request against it. Name the branch after the type of the
change, such as `fix/short-name` or `feature/123-short-name`. Sign off every commit with
`git commit -s`, which certifies the [Developer Certificate of Origin](https://developercertificate.org),
and sign it with your key: `main` takes signed commits only.

## Licenses

Kudari is under the [MIT license](LICENSE). By contributing, you agree to publish your
contribution under it. Every new file starts with an SPDX header naming you and the license,
as the existing files do, or has an entry in `REUSE.toml`. `reuse lint` checks that.

Please contribute only work you may publish under MIT. In particular, Kudari contains no
code of Yakuake or Konsole, which are under the GPL: their behavior may serve as a
reference, but do not copy their code.

## Releases

The maintainers release with `tools/release.py`, as [docs/releases.md](docs/releases.md)
describes.
