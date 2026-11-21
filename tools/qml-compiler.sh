#!/bin/sh
# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
# Tells which bindings and functions of the QML the QML compiler does not turn into C++: they
# run in the interpreter of QML instead, which is slower. A report to compare before and after
# a change of the QML, not a check: it fails on nothing.
#
#   tools/qml-compiler.sh [--list]
#
# It counts them for each file, the most first; --list also names each one. It needs a build
# in the target directory, as tests/check.sh does, for the QML modules of cxx-qt.
set -eu
cd "$(dirname "$0")/.."
target=${CARGO_TARGET_DIR:-target}
list=
case ${1:-} in
    --list) list=1 ;;
    "") ;;
    *) echo "Usage: tools/qml-compiler.sh [--list]" >&2; exit 2 ;;
esac
qmllint=$("${QMAKE:-qmake6}" -query QT_HOST_BINS 2>/dev/null)/qmllint
[ -x "$qmllint" ] || qmllint=/usr/lib/qt6/bin/qmllint
[ -d "$target/cxxqt/qml_modules" ] || { echo "No QML modules in $target; build Kudari first." >&2; exit 1; }
found=$(mktemp)
resources=$(mktemp -d)
trap 'rm -rf "$found" "$resources"' EXIT
# The settings pages import qrc:/qml/form, which each front end fills with its own parts; as in
# tests/check.sh, with those of KDE.
{
    echo '<RCC><qresource prefix="/">'
    for file in qml/form/*.qml; do
        echo "<file alias=\"qml/form/$(basename "$file")\">$(pwd)/$file</file>"
    done
    echo '</qresource></RCC>'
} > "$resources/forms.qrc"
env -u QT_LOGGING_RULES "$qmllint" --compiler info -I "$target/cxxqt/qml_modules" -I /usr/lib/qt6/qml \
    --resource "$resources/forms.qrc" qml/*.qml qml/form/*.qml crates/qt/qml/*.qml 2>&1 |
    grep '\[compiler\]$' > "$found" || true
if [ -n "$list" ]; then
    sed 's/^[A-Za-z]*: //; s/ \[compiler\]$//' "$found"
    echo
fi
echo "Not compiled to C++, for each file:"
sed -n 's/^[A-Za-z]*: \([^:]*\):.*/\1/p' "$found" | sort | uniq -c | sort -rn
echo "$(wc -l < "$found") in all"
