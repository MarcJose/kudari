# Instructions for coding agents

Instructions for AI coding agents working in this repository. People who contribute: see
[CONTRIBUTING.md](CONTRIBUTING.md), which holds the details that this file sums up.

## Ground rules

- Work on a branch created from `develop`. Never push to `main` or `develop` directly;
  always open a pull request. A person must review and approve it.
- Keep changes minimal and focused on the task. Do not refactor unrelated code.
- Never commit secrets, credentials, tokens, personal or customer data, not even in tests,
  fixtures, logs or examples. Use obvious placeholders.
- Do not add dependencies or GitHub Actions without a justification in the pull request.
  Pin GitHub Actions to a full commit SHA.
- Do not change files that concern security or governance (`.github/`, `SECURITY.md`,
  `LICENSE`, `REUSE.toml`, `deny.toml`, the CI configuration) unless the task requires it.
- Do not turn off, skip or weaken tests, linters or security checks to make a build pass.
- Use Conventional Commits, as `tools/commit-subject.sh` checks them, and sign off commits
  (`git commit -s`).
- Turn on the Git hooks with `git config core.hooksPath .githooks`; they run the checks
  before each commit and push.

## Project specifics

- Build: `cargo build`; with the source in a synced folder, set
  `CARGO_TARGET_DIR=~/.cache/kudari-target` first.
- Test: `tests/check.sh` (formatting, lints, unit tests, translations, prose, licenses);
  `cargo build --features test-hooks && tests/gui/run.sh` for the window and the terminal.
- Lint and format: `cargo fmt`, and clippy and the C++ compiler with warnings as errors, all
  run by `tests/check.sh`; `tools/security.sh` for secrets, dependencies and workflows.
- Project structure: `crates/core` holds what depends on no user interface and no platform,
  `crates/qt` the parts that need Qt but not KDE, the package at the root the KDE front end,
  and `crates/windows` the Windows front end. `docs/architecture.md` describes the layout.
- Texts: every text that the user sees needs a German and a British English translation in
  `po/`; `tools/translations.py` shows what is missing.
