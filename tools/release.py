#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
"""Prepares a release, or checks that the version is the same everywhere.

    tools/release.py [VERSION] [--dry-run]   raise the version, write the changelog, tag
    tools/release.py --check [--tag vX.Y.Z]  the version agrees everywhere (and with a tag)
    tools/release.py --notes VERSION         the changelog section of a release

Cargo.toml holds the version; CMake and the program read it from there. This tool also
writes it into Cargo.lock, the release list of the metainfo and the man page, and the
section of the release into CHANGELOG.md. Without VERSION, git-cliff picks the next one from
the Conventional Commits since the last tag; the first release keeps the version of
Cargo.toml. A changelog section that exists already, written by hand, is kept and dated.
The release is committed and tagged, but not pushed.

A pre-release such as 1.2.0-rc.1 takes the notes of the section of 1.2.0 if there is one,
which stays unreleased, and enters the metainfo as 1.2.0~rc.1 of the type development, as
AppStream orders versions with a tilde before the release.
"""

import argparse
import datetime
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CARGO = ROOT / "Cargo.toml"
LOCK = ROOT / "Cargo.lock"
METAINFO = ROOT / "data" / "io.github.marcjose.kudari.metainfo.xml"
MAN = ROOT / "docs" / "man-kudari.1.docbook"
CHANGELOG = ROOT / "CHANGELOG.md"
WORKFLOWS = ROOT / ".github" / "workflows"

SEMVER = re.compile(r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?$")
CARGO_VERSION = re.compile(r'^version = "([^"]+)"', re.MULTILINE)
LOCK_VERSION = re.compile(r'^name = "kudari"\nversion = "([^"]+)"', re.MULTILINE)
RELEASE = re.compile(r'<release version="([^"]+)" date="([^"]+)"(?: type="[a-z]+")?\s*/>')
RELEASEINFO = re.compile(r"<releaseinfo>([^<]*)</releaseinfo>")
MAN_DATE = re.compile(r"<date>([^<]*)</date>")
SECTION = re.compile(r"^## \[([^\]]+)\](?: - (\S+))?$", re.MULTILINE)
# Pinned images older than this are worth renewing before a release.
IMAGE_AGE_DAYS = 30


def run(*command, capture=True):
    result = subprocess.run(command, cwd=ROOT, text=True, capture_output=capture)
    if result.returncode != 0:
        sys.exit(f"release: {' '.join(command)} failed:\n{result.stderr if capture else ''}")
    return result.stdout.strip() if capture else ""


def first(pattern, text, what):
    match = pattern.search(text)
    if not match:
        sys.exit(f"release: {what} not found")
    return match


def key(version):
    """Orders versions; a pre-release comes before its release."""
    major, minor, patch = (int(part) for part in SEMVER.match(version).groups())
    return (major, minor, patch, "-" not in version, version)


def base(version):
    """The release of a pre-release, or the version itself."""
    return version.split("-", 1)[0]


def appstream(version):
    """The version as the metainfo has it: a pre-release with a tilde."""
    return version.replace("-", "~", 1)


def state():
    """The version and date as each file has it."""
    metainfo = first(RELEASE, METAINFO.read_text(), "a release in the metainfo")
    man = MAN.read_text()
    section = SECTION.search(CHANGELOG.read_text())
    return {
        "Cargo.toml": first(CARGO_VERSION, CARGO.read_text(), "the version in Cargo.toml").group(1),
        "Cargo.lock": first(LOCK_VERSION, LOCK.read_text(), "kudari in Cargo.lock").group(1),
        "metainfo": metainfo.group(1).replace("~", "-", 1),
        "man page": first(RELEASEINFO, man, "<releaseinfo> in the man page").group(1),
        "changelog": section.group(1) if section else None,
        "metainfo date": metainfo.group(2),
        "man page date": first(MAN_DATE, man, "<date> in the man page").group(1),
        "changelog date": section.group(2) if section else None,
    }


def check(tag=None):
    """Returns the problems; with a tag, also those that keep it from being released."""
    found = state()
    version = found["Cargo.toml"]
    problems = []
    if not SEMVER.match(version):
        problems.append(f"{version} in Cargo.toml is no Semantic Version")
    # A pre-release may stand under the unreleased section of its release.
    borrowed = version != base(version) and found["changelog"] == base(version) and found["changelog date"] == "unreleased"
    for name in ("Cargo.lock", "metainfo", "man page", "changelog"):
        if found[name] != version and not (name == "changelog" and borrowed):
            problems.append(f"the {name} has version {found[name]}, Cargo.toml {version}")
    if found["man page date"] != found["metainfo date"]:
        problems.append(f"the man page is dated {found['man page date']}, the metainfo {found['metainfo date']}")
    dated = found["changelog date"] not in (None, "unreleased")
    if dated and found["changelog date"] != found["metainfo date"]:
        problems.append(f"the changelog is dated {found['changelog date']}, the metainfo {found['metainfo date']}")
    if tag is not None:
        if tag != f"v{version}":
            problems.append(f"the tag {tag} does not name version {version}")
        if not dated and not borrowed:
            problems.append(f"the changelog section of {version} has no date")
    return problems


def notes(version):
    """The body of the changelog section of a version; for a pre-release without one, that
    of its release."""
    text = CHANGELOG.read_text()
    sections = list(SECTION.finditer(text))
    for wanted in dict.fromkeys((version, base(version))):
        for index, section in enumerate(sections):
            if section.group(1) == wanted:
                end = sections[index + 1].start() if index + 1 < len(sections) else len(text)
                return text[section.end():end].strip() + "\n"
    sys.exit(f"release: CHANGELOG.md has no section for {version}")


def next_version(current, tags):
    if f"v{current}" not in tags:
        return current
    bumped = run("git", "cliff", "--bumped-version").removeprefix("v")
    if bumped == current:
        sys.exit("release: no commit since the last release would go into the changelog")
    return bumped


def changelog_section(version, date):
    text = CHANGELOG.read_text()
    if version != base(version) and any(s.group(1) == base(version) for s in SECTION.finditer(text)):
        return text
    existing = next((s for s in SECTION.finditer(text) if s.group(1) == version), None)
    if existing:
        return text[: existing.start()] + f"## [{version}] - {date}" + text[existing.end():]
    generated = run("git", "cliff", "--unreleased", "--tag", f"v{version}", "--strip", "all")
    generated = SECTION.sub(f"## [{version}] - {date}", generated, count=1)
    if not SECTION.search(generated):
        generated = f"## [{version}] - {date}\n\nNo changes worth noting."
    position = SECTION.search(text)
    position = position.start() if position else len(text)
    return text[:position] + generated.strip() + "\n\n" + text[position:]


def write(version, date):
    CARGO.write_text(CARGO_VERSION.sub(f'version = "{version}"', CARGO.read_text(), count=1))
    run("cargo", "update", "--workspace", "--offline", "--quiet")

    metainfo = METAINFO.read_text()
    top = first(RELEASE, metainfo, "a release in the metainfo")
    kind = ' type="development"' if version != base(version) else ""
    entry = f'<release version="{appstream(version)}" date="{date}"{kind}/>'
    # The entry of the release itself, before it is out, makes way for a pre-release.
    if top.group(1) in (appstream(version), base(version)):
        metainfo = metainfo[: top.start()] + entry + metainfo[top.end():]
    else:
        indent = metainfo[: top.start()].rsplit("\n", 1)[1]
        metainfo = metainfo[: top.start()] + f"{entry}\n{indent}" + metainfo[top.start():]
    METAINFO.write_text(metainfo)

    man = RELEASEINFO.sub(f"<releaseinfo>{version}</releaseinfo>", MAN.read_text(), count=1)
    MAN.write_text(MAN_DATE.sub(f"<date>{date}</date>", man, count=1))

    CHANGELOG.write_text(changelog_section(version, date))


def warn_about_images():
    today = datetime.date.today()
    for workflow in sorted(WORKFLOWS.glob("*.yml")):
        for stamp in sorted(set(re.findall(r"archlinux:base-(\d{8})", workflow.read_text()))):
            age = (today - datetime.datetime.strptime(stamp, "%Y%m%d").date()).days
            if age > IMAGE_AGE_DAYS:
                print(f"note: {workflow.name} pins an Arch image {age} days old; tools/update-pins.py renews it")


def release(requested, dry_run):
    if not dry_run and run("git", "status", "--porcelain"):
        sys.exit("release: commit or stash the changes first")
    problems = check()
    if problems:
        sys.exit("release: the version disagrees before the release:\n  " + "\n  ".join(problems))
    current = state()["Cargo.toml"]
    tags = set(run("git", "tag", "--list", "v*").split())
    version = requested or next_version(current, tags)
    if not SEMVER.match(version):
        sys.exit(f"release: {version} is no Semantic Version such as 1.2.3")
    if f"v{version}" in tags:
        sys.exit(f"release: the tag v{version} exists already")
    released = [tag[1:] for tag in tags if SEMVER.match(tag[1:])]
    # Before the release of the version in Cargo.toml, its pre-releases may come.
    unreleased = base(version) == current and f"v{current}" not in tags
    if (key(version) < key(current) and not unreleased) or any(key(version) <= key(old) for old in released):
        sys.exit(f"release: {version} is not newer than {current} and the released versions")
    branch = run("git", "branch", "--show-current")
    if branch != "main":
        print(f"note: releasing from {branch or 'a detached HEAD'}, not main")
    date = datetime.datetime.now(datetime.UTC).date().isoformat()

    if dry_run:
        print(f"Would release {version} on {date}.")
        return
    write(version, date)
    problems = check(f"v{version}")
    if problems:
        sys.exit("release: the files disagree after the update:\n  " + "\n  ".join(problems))
    files = [str(path.relative_to(ROOT)) for path in (CARGO, LOCK, METAINFO, MAN, CHANGELOG)]
    run("git", "add", *files)
    run("git", "commit", "--quiet", "-m", f"chore(release): {version}", capture=False)
    # Verbatim, or Git would take the headings of the notes for comments and drop them.
    message = f"Kudari {version}\n\n{notes(version)}"
    run("git", "tag", "--annotate", "--cleanup=verbatim", f"v{version}", "-m", message)
    print(f"Tagged v{version}. Look over the commit, then publish it with:")
    print(f"    git push origin {branch or 'HEAD'} v{version}")
    warn_about_images()


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("version", nargs="?", help="the version to release, such as 1.2.0")
    parser.add_argument("--dry-run", action="store_true", help="only say which version would be released")
    parser.add_argument("--check", action="store_true", help="check that the version agrees everywhere")
    parser.add_argument("--tag", help="with --check: the tag being released")
    parser.add_argument("--notes", metavar="VERSION", help="print the changelog section of VERSION")
    arguments = parser.parse_args()
    if arguments.check:
        problems = check(arguments.tag)
        for problem in problems:
            print(f"version: {problem}", file=sys.stderr)
        sys.exit(1 if problems else 0)
    if arguments.notes:
        sys.stdout.write(notes(arguments.notes))
        return
    release(arguments.version, arguments.dry_run)


if __name__ == "__main__":
    main()
