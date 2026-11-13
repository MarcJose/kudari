# AGENTS.md

Instructions for AI coding agents (e.g. GitHub Copilot, Claude Code, Codex)
working in this repository. Human contributors: see [CONTRIBUTING.md](CONTRIBUTING.md).

## Ground rules

- Work on a branch created from `develop`. Never push to `main` or `develop`
  directly; always open a pull request. A human must review and approve it.
- Keep changes minimal and focused on the task. Do not refactor unrelated code.
- Never commit secrets, credentials, tokens, personal or customer data –
  not even in tests, fixtures, logs or examples. Use obvious placeholders.
- Do not add dependencies or GitHub Actions without a justification in the PR
  description. Pin GitHub Actions to a full commit SHA.
- Do not modify security- or governance-relevant files (`.github/`, `SECURITY.md`,
  `LICENSE`, `CODEOWNERS`, CI/CD configuration) unless the task explicitly requires it.
- Do not disable, skip or weaken tests, linters or security checks to make a build pass.
- Use Conventional Commits and sign off commits (`git commit -s`).
- Run `pre-commit run --all-files` before committing, if available.
- Tick "AI assistance used" in the pull request checklist and name the tool. Never tick
  "AI-generated parts reviewed line by line by a human" – that is the human reviewer's job.

## Project specifics

> TODO: Fill in so agents can verify their changes.

- Build: `TODO`
- Test: `TODO`
- Lint/format: `TODO`
- Project structure: TODO
