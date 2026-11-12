#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
"""Renews the container images pinned in the workflows, which Dependabot does not see.

    tools/update-pins.py [--check]

The Arch image moves to the newest weekly base image, TruffleHog to its newest release;
each keeps a tag to read and the digest that decides. --check only reports what is behind.
"""

import argparse
import json
import re
import sys
import urllib.request
from pathlib import Path

WORKFLOWS = Path(__file__).resolve().parent.parent / ".github" / "workflows"
ARCH = re.compile(r"archlinux:base-[0-9.]+@sha256:[0-9a-f]{64}")
TRUFFLEHOG = re.compile(r"ghcr\.io/trufflesecurity/trufflehog:[0-9.]+@sha256:[0-9a-f]{64}")


def fetch(url, headers=None):
    request = urllib.request.Request(url, headers={"User-Agent": "kudari-update-pins", **(headers or {})})
    with urllib.request.urlopen(request, timeout=30) as response:
        return response.read(), response.headers


def newest_arch():
    data, _ = fetch("https://hub.docker.com/v2/repositories/library/archlinux/tags?page_size=100&name=base-")
    tags = [tag for tag in json.loads(data)["results"] if re.fullmatch(r"base-\d{8}\.\d+\.\d+", tag["name"])]
    tag = max(tags, key=lambda tag: tag["name"])
    return f"archlinux:{tag['name']}@{tag['digest']}"


def newest_trufflehog():
    data, _ = fetch("https://api.github.com/repos/trufflesecurity/trufflehog/releases/latest")
    version = json.loads(data)["tag_name"].removeprefix("v")
    token, _ = fetch("https://ghcr.io/token?scope=repository:trufflesecurity/trufflehog:pull")
    accept = "application/vnd.oci.image.index.v1+json, application/vnd.docker.distribution.manifest.list.v2+json"
    request = urllib.request.Request(
        f"https://ghcr.io/v2/trufflesecurity/trufflehog/manifests/{version}",
        method="HEAD",
        headers={"Authorization": f"Bearer {json.loads(token)['token']}", "Accept": accept},
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        digest = response.headers["Docker-Content-Digest"]
    return f"ghcr.io/trufflesecurity/trufflehog:{version}@{digest}"


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--check", action="store_true", help="only report pins that are behind")
    arguments = parser.parse_args()

    newest = {ARCH: newest_arch(), TRUFFLEHOG: newest_trufflehog()}
    behind = False
    for workflow in sorted(WORKFLOWS.glob("*.yml")):
        text = workflow.read_text()
        updated = text
        for pattern, pin in newest.items():
            for old in sorted(set(pattern.findall(text))):
                if old != pin:
                    behind = True
                    print(f"{workflow.name}: {old.split('@')[0]} -> {pin.split('@')[0]}")
                    updated = updated.replace(old, pin)
        if updated != text and not arguments.check:
            workflow.write_text(updated)
    if not behind:
        print("All pinned images are current.")
    sys.exit(1 if behind and arguments.check else 0)


if __name__ == "__main__":
    main()
