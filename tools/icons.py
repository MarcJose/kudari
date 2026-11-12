#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
"""Renders the icon of Kudari, data/icons/io.github.marcjose.kudari.svg, into what Windows
needs, as Windows builds have no renderer for SVG: the sizes for the window and the tray,
an .ico with all of them for the program, and the logos of the MSIX package. KDE installs
the SVG itself. Needs rsvg-convert (librsvg); run it after changing the SVG.

    tools/icons.py           write the files
    tools/icons.py --check   fail if they differ from what the SVG gives
"""

import argparse
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "data/icons/io.github.marcjose.kudari.svg"
ICONS = Path("crates/windows/icons")
ASSETS = Path("crates/windows/package/Assets")
# The sizes Windows asks for at 100 to 200 % scaling, and the large one of Explorer.
SIZES = [16, 20, 24, 32, 40, 48, 64, 96, 256]
# The logos the manifest names, with their edge lengths.
LOGOS = {
    "Square44x44Logo.png": 44,
    "Square44x44Logo.targetsize-44_altform-unplated.png": 44,
    "Square150x150Logo.png": 150,
    "StoreLogo.png": 50,
}


def render(size, path):
    subprocess.run(["rsvg-convert", "-w", str(size), "-h", str(size), "-o", str(path), str(SOURCE)], check=True)


def ico(images):
    """An .ico of PNG images, as Windows reads them since Vista: a header, one entry per
    image, then the images. 256 pixels are written as 0 in the entries."""
    header = struct.pack("<HHH", 0, 1, len(images))
    offset = len(header) + 16 * len(images)
    entries = b""
    data = b""
    for size, png in images:
        side = 0 if size >= 256 else size
        entries += struct.pack("<BBBBHHII", side, side, 0, 0, 1, 32, len(png), offset + len(data))
        data += png
    return header + entries + data


def write_all(out):
    """Every file, below `out` at the paths of the repository."""
    (out / ICONS).mkdir(parents=True, exist_ok=True)
    (out / ASSETS).mkdir(parents=True, exist_ok=True)
    images = []
    for size in SIZES:
        path = out / ICONS / f"kudari-{size}.png"
        render(size, path)
        images.append((size, path.read_bytes()))
    (out / ICONS / "kudari.ico").write_bytes(ico(images))
    for name, size in LOGOS.items():
        render(size, out / ASSETS / name)


def files():
    return [ICONS / f"kudari-{size}.png" for size in SIZES] + [ICONS / "kudari.ico"] + [ASSETS / name for name in LOGOS]


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--check", action="store_true", help="fail if the files differ from the SVG")
    arguments = parser.parse_args()
    if not arguments.check:
        write_all(ROOT)
        print(f"{len(files())} files from {SOURCE.relative_to(ROOT)}")
        return 0
    with tempfile.TemporaryDirectory() as directory:
        fresh = Path(directory)
        write_all(fresh)
        stale = [str(path) for path in files() if not (ROOT / path).is_file() or (ROOT / path).read_bytes() != (fresh / path).read_bytes()]
    if stale:
        print("out of date, run tools/icons.py: " + ", ".join(stale), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
