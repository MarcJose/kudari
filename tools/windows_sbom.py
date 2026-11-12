#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
"""Completes the CycloneDX SBOM of kudari-windows, which cargo-cyclonedx writes from the
crates, with what the Windows packages carry besides: the modules of Qt and the runtime of
Microsoft Visual C++, with the checksums of their files.

    tools/windows_sbom.py CRATES.cdx.json STAGE QT_VERSION RUNTIME_VERSION > OUT.cdx.json

STAGE is the directory of the package; the Qt modules are the Qt6*.dll in it.
"""

import hashlib
import json
import sys
from pathlib import Path

RUNTIME = ("msvcp140", "vcruntime140", "concrt140")


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def component(reference, name, version, licenses, path, supplier, website):
    return {
        "type": "library",
        "bom-ref": reference,
        "supplier": {"name": supplier},
        "name": name,
        "version": version,
        "scope": "required",
        "hashes": [{"alg": "SHA-256", "content": sha256(path)}],
        "licenses": licenses,
        "externalReferences": [{"type": "website", "url": website}],
    }


def main():
    if len(sys.argv) != 5:
        sys.exit(__doc__)
    crates, stage, qt_version, runtime_version = sys.argv[1:]
    sbom = json.loads(Path(crates).read_text(encoding="utf-8"))
    stage = Path(stage)
    added = []
    for path in sorted(stage.glob("Qt6*.dll")):
        name = path.stem
        added.append(
            component(
                f"qt:{name}@{qt_version}",
                name,
                qt_version,
                [{"license": {"id": "LGPL-3.0-only"}}],
                path,
                "The Qt Company",
                "https://www.qt.io/",
            )
        )
    for path in sorted(stage.glob("*.dll")):
        if path.stem.lower().startswith(RUNTIME):
            added.append(
                component(
                    f"msvc:{path.stem}@{runtime_version}",
                    path.stem,
                    runtime_version,
                    [{"license": {"name": "Microsoft Visual C++ Redistributable"}}],
                    path,
                    "Microsoft",
                    "https://learn.microsoft.com/cpp/windows/latest-supported-vc-redist",
                )
            )
    if not any(item["bom-ref"].startswith("qt:") for item in added):
        sys.exit(f"windows_sbom: no Qt in {stage}")
    sbom.setdefault("components", []).extend(added)
    root = sbom["metadata"]["component"]["bom-ref"]
    dependencies = sbom.setdefault("dependencies", [])
    entry = next((item for item in dependencies if item["ref"] == root), None)
    if entry is None:
        entry = {"ref": root, "dependsOn": []}
        dependencies.append(entry)
    entry.setdefault("dependsOn", []).extend(item["bom-ref"] for item in added)
    dependencies.extend({"ref": item["bom-ref"]} for item in added)
    json.dump(sbom, sys.stdout, indent=2)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
