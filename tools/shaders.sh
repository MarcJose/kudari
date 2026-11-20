#!/bin/sh
# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
#
# Bakes the shaders of crates/qt/shaders into the .qsb files next to them, with the variants
# for every backend of Qt Quick: builds take those and need no Qt Shader Tools. In the format
# of Qt 6.5, which every Qt that Kudari builds with reads.
#
#     tools/shaders.sh           bakes them
#     tools/shaders.sh --check   tells whether they match their sources, as tests/check.sh asks
set -eu

cd "$(dirname "$0")/.."
qsb=$(command -v qsb6 || command -v qsb || echo /usr/lib/qt6/bin/qsb)
[ -x "$qsb" ] || { echo "qsb of Qt Shader Tools 6 is missing" >&2; exit 1; }
check=false
[ "${1:-}" = --check ] && check=true
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
status=0
for source in crates/qt/shaders/*.vert crates/qt/shaders/*.frag; do
    "$qsb" --qt6 -b --qsbversion 65 -o "$work/out.qsb" "$source"
    if $check; then
        [ "$(sha256sum < "$work/out.qsb")" = "$(sha256sum < "$source.qsb")" ] || { echo "$source.qsb does not match $source: run tools/shaders.sh" >&2; status=1; }
    else
        cp "$work/out.qsb" "$source.qsb"
    fi
done
exit $status
