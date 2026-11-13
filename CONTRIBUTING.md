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
with it. The checks need [Vale](https://vale.sh) as well, for the English of the texts.

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
  the English of the documentation and the comments against the Google developer
  documentation style, with the exceptions in `.vale.ini`. Words of the project that it does
  not know go into `.vale/styles/config/vocabularies/Kudari/accept.txt`.
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
```

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
