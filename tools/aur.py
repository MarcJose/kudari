#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
"""Keeps the packages for the AUR in packaging/aur up to date.

    tools/aur.py VERSION [--sums SHA256SUMS]   kudari-bin to a release, and every .SRCINFO
    tools/aur.py --check                       every .SRCINFO agrees with its PKGBUILD

kudari-bin installs the package of a release, kudari-git builds the newest state of main.
After a release, VERSION sets the version of kudari-bin and the checksum of its package,
from the SHA256SUMS of the release (downloaded with gh unless --sums names the file). Only
releases count, not pre-releases such as 1.2.0-rc.1, which pacman would order wrongly.
Publishing is a push of a package's directory to ssh://aur@aur.archlinux.org/NAME.git.
"""

import argparse
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
AUR = ROOT / "packaging" / "aur"
BIN = AUR / "kudari-bin" / "PKGBUILD"
RELEASE = re.compile(r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$")


def srcinfo(directory):
    """What makepkg makes of the PKGBUILD in directory. makepkg refuses to run as root and
    wants to write next to the PKGBUILD, so it reads a copy in a directory of its own."""
    command = ["makepkg", "--printsrcinfo"]
    if os.geteuid() == 0:
        command = ["runuser", "-u", "nobody", "--", *command]
    with tempfile.TemporaryDirectory() as build:
        os.chmod(build, 0o777)
        copy = Path(build) / "PKGBUILD"
        shutil.copyfile(directory / "PKGBUILD", copy)
        copy.chmod(0o644)
        result = subprocess.run(command, cwd=build, text=True, capture_output=True)
    if result.returncode != 0:
        sys.exit(f"aur: makepkg --printsrcinfo failed for {directory.name}:\n{result.stderr}")
    return result.stdout


def checksum(version, sums):
    name = f"kudari-{version}-x86_64-linux.tar.xz"
    if sums:
        text = Path(sums).read_text()
    else:
        text = subprocess.run(
            ["gh", "release", "download", f"v{version}", "--pattern", "SHA256SUMS", "--output", "-"],
            cwd=ROOT, text=True, capture_output=True, check=True,
        ).stdout
    for line in text.splitlines():
        parts = line.split()
        if len(parts) == 2 and parts[1].lstrip("*") == name:
            return parts[0]
    sys.exit(f"aur: {name} is not in SHA256SUMS")


def update(version, sums):
    if not RELEASE.match(version):
        sys.exit(f"aur: {version} is no release; kudari-bin follows releases only")
    text = BIN.read_text()
    text = re.sub(r"^pkgver=.*$", f"pkgver={version}", text, count=1, flags=re.MULTILINE)
    text = re.sub(r"^pkgrel=.*$", "pkgrel=1", text, count=1, flags=re.MULTILINE)
    text = re.sub(r"^sha256sums_x86_64=\('[0-9a-f]*'\)$", f"sha256sums_x86_64=('{checksum(version, sums)}')",
                  text, count=1, flags=re.MULTILINE)
    BIN.write_text(text)
    for package in packages():
        (package / ".SRCINFO").write_text(srcinfo(package))
    print(f"kudari-bin is at {version}. Publish each package with a push of its directory to")
    print("ssh://aur@aur.archlinux.org/<name>.git, with PKGBUILD and .SRCINFO.")


def packages():
    return sorted(path for path in AUR.iterdir() if (path / "PKGBUILD").is_file())


def check():
    stale = [p.name for p in packages() if (p / ".SRCINFO").read_text() != srcinfo(p)]
    if stale:
        sys.exit(f"aur: .SRCINFO is out of date in {', '.join(stale)}; run tools/aur.py with the version")
    print(f"{len(packages())} packages for the AUR, .SRCINFO up to date")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("version", nargs="?")
    parser.add_argument("--sums", help="the SHA256SUMS of the release, instead of downloading it")
    parser.add_argument("--check", action="store_true")
    arguments = parser.parse_args()
    if arguments.check:
        check()
    elif arguments.version:
        update(arguments.version, arguments.sums)
    else:
        parser.error("a version or --check")


if __name__ == "__main__":
    main()
