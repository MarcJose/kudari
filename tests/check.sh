#!/bin/sh
# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
# Everything that has to pass before a commit. Used by the CI as well.
set -eu

cd "$(dirname "$0")/.."
target=${CARGO_TARGET_DIR:-target}

echo "== Version"
python3 tools/release.py --check

echo "== Third-party licenses"
python3 tools/third_party_licenses.py --check

echo "== Generated tables"
python3 tools/box_drawing.py --check

echo "== Translations"
python3 tools/translations.py --check --strict --quiet

echo "== Packages for the AUR"
python3 tools/aur.py --check
for pkgbuild in packaging/aur/*/PKGBUILD; do
    findings=$(namcap "$pkgbuild")
    if [ -n "$findings" ]; then
        echo "$findings"
        exit 1
    fi
done

echo "== Formatting"
cargo fmt --all --check

echo "== Rust lints"
cargo clippy --locked --workspace --all-targets --features test-hooks -- -D warnings

echo "== Unit tests"
cargo test --locked --workspace

echo "== C++ warnings"
# The headers of Qt and KDE are system headers here: only our own code counts.
# The newest generated header: other builds in the target directory can be older.
header=$(find "$target" -path '*/kudari/src/kde.cxx.h' -printf '%T@ %p\n' | sort -n | tail -n 1 | cut -d' ' -f2-)
generated=$(dirname "$(dirname "$(dirname "$header")")")
includes="-isystem /usr/include/qt6"
for module in /usr/include/qt6/Qt* /usr/include/KF6/*; do
    [ -d "$module" ] && includes="$includes -isystem $module"
done
# shellcheck disable=SC2086 # the include list is meant to split
g++ -std=c++17 -fPIC -fsyntax-only -Icpp "-I$generated" -DQT_GUI_LIB -DQT_WIDGETS_LIB -DQT_CORE_LIB $includes \
    -Wall -Wextra -Wshadow -Wconversion -Wsign-conversion -Wold-style-cast -Wnon-virtual-dtor \
    -Woverloaded-virtual -Wno-unused-parameter -Werror cpp/kdebridge.cpp
header=$(find "$target" -path '*/kudari-qt/src/qt.cxx.h' -printf '%T@ %p\n' | sort -n | tail -n 1 | cut -d' ' -f2-)
generated=$(dirname "$(dirname "$(dirname "$header")")")
# shellcheck disable=SC2086 # the include list is meant to split
g++ -std=c++17 -fPIC -fsyntax-only -Icrates/qt/cpp "-I$generated" -DQT_GUI_LIB -DQT_WIDGETS_LIB -DQT_CORE_LIB $includes \
    -Wall -Wextra -Wshadow -Wconversion -Wsign-conversion -Wold-style-cast -Wnon-virtual-dtor \
    -Woverloaded-virtual -Wno-unused-parameter -Werror crates/qt/cpp/qtbridge.cpp
header=$(find "$target" -path '*/kudari-windows/src/win.cxx.h' -printf '%T@ %p\n' | sort -n | tail -n 1 | cut -d' ' -f2-)
generated=$(dirname "$(dirname "$(dirname "$header")")")
# shellcheck disable=SC2086 # the include list is meant to split
g++ -std=c++17 -fPIC -fsyntax-only -Icrates/windows/cpp "-I$generated" -DQT_GUI_LIB -DQT_WIDGETS_LIB -DQT_CORE_LIB $includes \
    -Wall -Wextra -Wshadow -Wconversion -Wsign-conversion -Wold-style-cast -Wnon-virtual-dtor \
    -Woverloaded-virtual -Wno-unused-parameter -Werror crates/windows/cpp/winbridge.cpp

echo "== QML"
# Qt's linter cannot see into the types of cxx-qt, so these categories only give false alarms.
# `pragma ComponentBehavior: Bound` and the QML compiler in the build cover part of them.
qmllint() {
    env -u QT_LOGGING_RULES /usr/lib/qt6/bin/qmllint --max-warnings 0 \
        --import disable --unresolved-type disable --missing-property disable --missing-type disable \
        --incompatible-type disable --unqualified disable \
        -I "$target/cxxqt/qml_modules" -I /usr/lib/qt6/qml "$@"
}
qmllint qml/*.qml qml/form/*.qml crates/windows/qml/form/*.qml
# The settings pages import qrc:/qml/form, which each front end fills with the parts of its
# forms; they are checked against those of both, and the QML of Windows against its own.
resources=$(mktemp -d)
trap 'rm -rf "$resources"' EXIT
for crate in . crates/windows; do
    qrc="$resources/$(basename "$(cd "$crate" && pwd)").qrc"
    {
        echo '<RCC><qresource prefix="/">'
        for file in "$crate"/qml/form/*.qml; do
            echo "<file alias=\"qml/form/$(basename "$file")\">$(pwd)/$file</file>"
        done
        echo '</qresource></RCC>'
    } > "$qrc"
    if [ "$crate" = crates/windows ]; then
        qmllint --resource "$qrc" crates/qt/qml/*.qml crates/windows/qml/*.qml
    else
        qmllint --resource "$qrc" crates/qt/qml/*.qml
    fi
done

echo "== Prose"
# The English of the documentation and of the comments, as .vale.ini describes.
git ls-files -z '*.md' '*.rs' '*.cpp' '*.h' '*.py' '*.qml' '*.ps1' ':(exclude,glob)third_party/*/**' | xargs -0 vale --no-wrap

echo "== Man page"
checkXML6 docs/man-kudari.1.docbook
# Its English as well, turned into HTML with the entities of KDocTools resolved. xsltproc
# reports an entity it cannot resolve but still succeeds, so any message of it fails here.
man=$(mktemp -d)
XML_CATALOG_FILES=/usr/share/kf6/kdoctools/customization/catalog.xml \
    xsltproc --nonet .vale/docbook.xsl docs/man-kudari.1.docbook > "$man/page.html" 2> "$man/errors"
if [ -s "$man/errors" ]; then
    cat "$man/errors"
    exit 1
fi
vale --ext=.html --no-wrap < "$man/page.html"
rm -r "$man"

echo "All checks passed."
