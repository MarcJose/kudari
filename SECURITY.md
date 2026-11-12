# Security

A terminal sees everything that is typed into it and runs what it is told, so a flaw in
Kudari can expose a lot. Reports are welcome and are taken seriously.

## Reporting a vulnerability

Please report it privately, not in a public issue or pull request:

- through GitHub, under
  [Security → Report a vulnerability](https://github.com/MarcJose/kudari/security/advisories/new),
  which is preferred, or
- by mail to Marc Jose, [marc@hahn-jose.de](mailto:marc@hahn-jose.de).

Helpful are the version (`kudari --version`), how Kudari was installed, the steps or input
that show the problem, and what an attacker gains by it. A rough report is better than none.

You can expect an answer within a week. Once the problem is confirmed, it is fixed in a new
release, and a GitHub security advisory describes it, with a CVE where one fits. Unless you
prefer otherwise, the advisory and the changelog name you. Please keep the details private
until the release is out; if a fix takes longer than 90 days, we agree on a date together.

## Supported versions

Fixes go into the newest release only. Until 1.0 there are no fixes for older versions, so
please update before reporting.

## What counts

Among others, anything that lets a program, a file or a website do more than the user
allowed:

- Output, pasted text or a file name that makes the terminal run commands, change its
  settings or leave the bracketed paste.
- The D-Bus interface accepting commands while `runCommand` is switched off, or doing more
  than the documentation says.
- An AI agent reading or controlling terminals against the settings under Settings →
  Security → AI Agents or without the user allowing it in Kudari's question, keeping its
  access after it disconnected or the user ended it, running a command without the user's
  Return while they are to be confirmed, or closing a session it did not open.
- Links or paths that start programs or open schemes not allowed under Settings → Terminal
  Behavior.
- Reading the clipboard through OSC 52, or other control sequences that reveal data.
- The import of Yakuake's and Konsole's settings writing to their files or running anything.
- Release files whose signature, checksum or attestation can be forged, or a release
  workflow that can be made to publish something else.

Not a vulnerability of Kudari is what a program running in the terminal does with the rights
the user gave it, or a flaw in Qt, KDE Frameworks or a Rust crate; please report those to
their projects, and tell us as well if Kudari should work around it.

## Checking a release

Every release is signed through Sigstore and comes with SHA256 and SHA512 checksums, SBOMs
and build provenance; [docs/releases.md](docs/releases.md) describes how to verify them and
how to rebuild the package byte for byte.
