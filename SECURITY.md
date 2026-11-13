# Security

A terminal sees everything you type into it and runs what it receives, so a flaw in Kudari
can expose a lot. Reports are welcome, and each one gets serious attention.

## Reporting a vulnerability

Please report it privately, not in a public issue or pull request:

- through GitHub, under
  [Security → Report a vulnerability](https://github.com/MarcJose/kudari/security/advisories/new),
  the preferred way, or
- by mail to Marc Jose, [marc@hahn-jose.de](mailto:marc@hahn-jose.de).

Helpful are the version (`kudari --version`), how you installed Kudari, the steps or input
that show the problem, and what an attacker gains by it. A rough report is better than none.

You can expect an answer within a week. Once the problem is clear, a new release fixes it,
and a GitHub security advisory describes it, with a CVE where one fits. Unless you prefer
otherwise, the advisory and the changelog name you. Please keep the details private until
the release is out. If a fix takes longer than 90 days, you and the maintainer agree on a
date together.

## Supported versions

Fixes go into the newest release only. Until 1.0 there are no fixes for older versions, so
please update before reporting.

## What counts

Among others, anything that lets a program, a file or a website do more than the user
allowed:

- Output, pasted text or a filename that makes the terminal run commands, change its
  settings or leave the bracketed paste.
- The D-Bus interface accepting commands while `runCommand` is off, or doing more
  than the documentation says.
- An AI agent reading or controlling terminals against the settings under Settings →
  Security → AI Agents or without the user allowing it in Kudari's question, keeping its
  access after it disconnected or the user ended it, running a command without the user's
  Return while they wait for it, or closing a session it did not open.
- Links or paths that start programs or open schemes not allowed under Settings → Terminal
  Behavior.
- Reading the clipboard through OSC 52, or other control sequences that reveal data.
- The import of Yakuake's and Konsole's settings writing to their files or running anything.
- Release files with a signature, checksum or attestation that someone can forge, or a
  release workflow that someone can trick into publishing something else.

Not a vulnerability of Kudari is what a program running in the terminal does with the rights
the user gave it, or a flaw in Qt, KDE Frameworks or a Rust crate. Please report those to
their projects, and report it here as well if Kudari should work around it.

## Checking a release

Every release carries a signature through Sigstore, SHA256 and SHA512 checksums, SBOMs and
build provenance. [docs/releases.md](docs/releases.md) describes how to verify them and how
to rebuild the package byte for byte.
