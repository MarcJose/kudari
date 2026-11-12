#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
"""Writes the winget manifests of a release, for a pull request to microsoft/winget-pkgs.

    tools/winget.py VERSION ZIP OUTDIR

winget installs the ZIP archive as a portable program, as the MSIX package is signed only
by Kudari itself, which winget does not trust. The program finds its Qt libraries next to
itself, so winget puts its directory on the path instead of linking the program alone.
"""

import hashlib
import re
import sys
from pathlib import Path

IDENTIFIER = "MarcJose.Kudari"
SCHEMA = "1.9.0"
ROOT = Path(__file__).resolve().parent.parent


def repository():
    text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    found = re.search(r'^repository = "(.*)"', text, re.MULTILINE)
    if not found:
        sys.exit("winget: Cargo.toml names no repository")
    return found.group(1)


def manifest(kind, version, body):
    schema = f"https://aka.ms/winget-manifest.{kind}.{SCHEMA}.schema.json"
    return (
        f"# yaml-language-server: $schema={schema}\n"
        f"PackageIdentifier: {IDENTIFIER}\n"
        f"PackageVersion: {version}\n"
        f"{body}"
        f"ManifestType: {kind}\n"
        f"ManifestVersion: {SCHEMA}\n"
    )


def main():
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    version, archive, out = sys.argv[1], Path(sys.argv[2]), Path(sys.argv[3])
    url = repository()
    checksum = hashlib.sha256(archive.read_bytes()).hexdigest().upper()
    program = f"kudari-{version}\\kudari.exe"
    files = {
        f"{IDENTIFIER}.yaml": manifest("version", version, "DefaultLocale: en-US\n"),
        f"{IDENTIFIER}.installer.yaml": manifest(
            "installer",
            version,
            "MinimumOSVersion: 10.0.17763.0\n"
            "InstallerType: zip\n"
            "NestedInstallerType: portable\n"
            "NestedInstallerFiles:\n"
            f"  - RelativeFilePath: {program}\n"
            "ArchiveBinariesDependOnPath: true\n"
            "Commands:\n"
            "  - kudari\n"
            "Installers:\n"
            "  - Architecture: x64\n"
            f"    InstallerUrl: {url}/releases/download/v{version}/{archive.name}\n"
            f"    InstallerSha256: {checksum}\n",
        ),
        f"{IDENTIFIER}.locale.en-US.yaml": manifest(
            "defaultLocale",
            version,
            "PackageLocale: en-US\n"
            "Publisher: Marc Jose\n"
            "PackageName: Kudari\n"
            f"PackageUrl: {url}\n"
            "License: MIT\n"
            f"LicenseUrl: {url}/blob/v{version}/LICENSES/MIT.txt\n"
            "ShortDescription: A drop-down terminal, in the tradition of Yakuake\n"
            "Description: |-\n"
            "  Kudari slides down from the top of the screen on a shortcut, Ctrl+F12 by default,\n"
            "  with tabs and split terminals for the Command Prompt, PowerShell and WSL. AI agents\n"
            "  can read its terminals through the Model Context Protocol if the user allows it.\n"
            "Moniker: kudari\n"
            "Tags:\n"
            "  - terminal\n"
            "  - drop-down\n"
            "  - quake\n"
            "  - powershell\n"
            "  - wsl\n"
            f"ReleaseNotesUrl: {url}/releases/tag/v{version}\n",
        ),
    }
    out.mkdir(parents=True, exist_ok=True)
    for name, text in files.items():
        (out / name).write_text(text, encoding="utf-8", newline="\n")
        print(out / name)


if __name__ == "__main__":
    main()
